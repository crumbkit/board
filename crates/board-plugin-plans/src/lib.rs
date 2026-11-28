//! The plans plugin built into Board (plugins/plans/README.md).

mod answer;
mod ask;
mod decide;
mod fill;
mod migrate;
mod open;
mod question;
pub mod records;
mod stale;
mod sweep;
mod waiting;
mod withdraw;

use board_model::{CapabilityName, RecordType};
use board_ops::{Host, Plugin, PluginError};
use board_plugin::Manifest;
use board_plugin::shape::{Record, schema_of};
use schemars::JsonSchema;
use schemars::generate::SchemaSettings;
use serde_json::{Map, Value, json};

use crate::records::{AnswerRecord, DecisionRecord, QuestionRecord};

#[derive(Clone, Copy, Debug)]
pub struct Plans;

impl Plugin for Plans {
    fn manifest(&self) -> Manifest {
        Manifest::built_in(manifest())
            .expect("the manifest is built from constants and its types' schemas; a test parses it")
    }

    fn schemas(&self) -> Vec<(RecordType, Value)> {
        record_schemas()
            .into_iter()
            .map(|(name, schema)| {
                let name = name
                    .parse()
                    .expect("each record's TYPE is a record type; a test parses it");
                (name, schema)
            })
            .collect()
    }

    fn fills(&self) -> &'static [board_ops::Fill] {
        &fill::FILLS
    }

    fn call(
        &self,
        host: &dyn Host,
        capability: &CapabilityName,
        input: Value,
    ) -> Result<Value, PluginError> {
        match capability.as_str() {
            "open" => open::open(host, &input),
            "answer" => answer::answer(host, &input),
            "withdraw" => withdraw::withdraw(host, &input),
            "migrate" => migrate::migrate(host, &input),
            "waiting" => waiting::waiting(host, &input),
            "sweep" => sweep::sweep(host, &input),
            "stale" => stale::stale(host, &input),
            "ask" => ask::ask(host, &input),
            "decide" => decide::decide(host, &input),
            other => Err(PluginError {
                code: "plans::call::unknown_capability".to_owned(),
                message: format!(
                    "plans offers no capability {other}; it offers open, answer, withdraw, migrate, waiting, sweep, ask, stale and decide"
                ),
                fields: Map::new(),
                hint: None,
            }),
        }
    }
}

/// What a capability does to the board. Every capability reads and writes
/// only the board, so each is closed-world.
#[derive(Clone, Copy)]
enum Effect {
    /// It writes nothing.
    Reads,
    /// It only adds records, and a repeat adds them again.
    Adds,
    /// It only adds records or fields, and a repeat adds nothing more.
    AddsOnce,
    /// It changes fields of existing records, and a repeat changes nothing
    /// more.
    OverwritesOnce,
}

impl Effect {
    /// MCP's read-only, destructive and idempotent hints for it.
    fn hints(self) -> [bool; 3] {
        match self {
            Self::Reads => [true, false, true],
            Self::Adds => [false, false, false],
            Self::AddsOnce => [false, false, true],
            Self::OverwritesOnce => [false, true, true],
        }
    }
}

/// One capability's manifest entry.
fn capability<I: JsonSchema, O: JsonSchema>(
    name: &str,
    description: &str,
    effect: Effect,
) -> Value {
    let [read_only, destructive, idempotent] = effect.hints();
    json!({
        "name": name,
        "description": description,
        "inputSchema": schema_of::<I>(SchemaSettings::draft2020_12()),
        "outputSchema": schema_of::<O>(SchemaSettings::draft2020_12().for_serialize()),
        "annotations": {
            "readOnlyHint": read_only,
            "destructiveHint": destructive,
            "idempotentHint": idempotent,
            "openWorldHint": false,
        },
        "_meta": {},
    })
}

fn manifest() -> Value {
    json!({
        "$type": "board/plugin-manifest/v1",
        "name": "plans",
        "version": "0.1.0",
        "description": "Records a session's decisions, each question for the owner, and the \
            owner's answers. Use it when you choose between options a later session needs to know \
            about, or when work waits on the owner.",
        "metadata": {"area": "planning"},
        "types": record_schemas().map(|(name, _)| name),
        "capabilities": [
            capability::<open::Input, open::Output>(
                "open",
                "The questions under a path whose status is open and that no answer names, \
                 grouped by project: a project holding a question that blocks active work comes \
                 first, and within a project those questions come first, each oldest first.",
                Effect::Reads,
            ),
            capability::<answer::Input, answer::Output>(
                "answer",
                "Record the owner's answer to an open question. The answer is what marks the \
                 question answered; the question's content is left unchanged.",
                Effect::AddsOnce,
            ),
            capability::<withdraw::Input, withdraw::Output>(
                "withdraw",
                "Withdraw an open question that no answer names, with the reason, so that \
                 plans.open stops listing it.",
                Effect::OverwritesOnce,
            ),
            capability::<migrate::Input, migrate::Output>(
                "migrate",
                "Rewrite each question whose blocks holds words: a value that names one change \
                 becomes that change's path and its words move to blocked_work, and any other \
                 value moves to blocked_work. apply writes every line in one commit with a \
                 Board-Bulk trailer; without it nothing is written.",
                Effect::OverwritesOnce,
            ),
            capability::<waiting::Input, waiting::Output>(
                "waiting",
                "How many questions under a path wait for the owner, and one line that says so, \
                 which an agent can end its reply with.",
                Effect::Reads,
            ),
            capability::<sweep::Input, sweep::Output>(
                "sweep",
                "Withdraw, with a reason, each open question whose blocked change or task is done \
                 or dropped, whose asking session and every also_by session are archived, or \
                 whose words a question answered in its project already asked. apply writes each \
                 board's withdrawals in one commit with a Board-Bulk trailer; without it nothing \
                 is written.",
                Effect::OverwritesOnce,
            ),
            capability::<ask::Input, ask::Output>(
                "ask",
                "File a question for the owner under /projects/<project>/questions/, unless an \
                 open question in the project asks the same words, which then takes the asker \
                 into its also_by, or a question answered in the project asked them, whose \
                 answer and decisions are returned instead.",
                Effect::AddsOnce,
            ),
            capability::<stale::Input, stale::Output>(
                "stale",
                "List the changes in plan with no owner, the evals planned or running with no \
                 verdict, and the open tasks with no owner that show no activity for more than \
                 days (7), or task_days (30) for a task. apply asks the owner about each with a \
                 drop-or-keep question through plans.ask, as session; without it nothing is \
                 written.",
                Effect::AddsOnce,
            ),
            capability::<decide::Input, decide::Output>(
                "decide",
                "Record a decision under /projects/<project>/decisions/, filling in its $type, \
                 decided as now, and by from session, person and program. A call that names \
                 neither a session nor a person is refused. A project that held nothing before \
                 is still written to, with a warning that names the three nearest projects. A \
                 repeat records a second decision.",
                Effect::Adds,
            ),
        ],
    })
}

/// Each record type the plugin ships, with the schema its struct generates.
fn record_schemas() -> [(&'static str, Value); 3] {
    let settings = SchemaSettings::draft2020_12;
    [
        (
            QuestionRecord::TYPE,
            schema_of::<QuestionRecord>(settings()),
        ),
        (AnswerRecord::TYPE, schema_of::<AnswerRecord>(settings())),
        (
            DecisionRecord::TYPE,
            schema_of::<DecisionRecord>(settings()),
        ),
    ]
}

#[cfg(test)]
mod tests;
