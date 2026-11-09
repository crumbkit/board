//! The changes plugin built into Board (plugins/changes/README.md): it
//! walks one change to a mounted repository from its plan to its pull
//! request, gates each phase on the records the change references, keeps
//! work in progress under a soft limit, and ends a change when its pull
//! request closes. Tasks are written through the tasks plugin and controls
//! read through the controls plugin, each by a nested call.

mod board;
mod brief;
mod change;
mod controls;
mod diff;
mod flow;
mod gates;
mod migrate;
mod open;
mod phases;
mod plan;
mod pr;
mod prototype;
mod records;
mod repo;
mod run;
mod start;
mod stop;
mod surfaced;
mod sync;
mod tasks;
mod wip;

pub use change::attached;

use board_model::{CapabilityName, RecordType};
use board_ops::{Host, Plugin, PluginError};
use board_plugin::Manifest;
use board_plugin::shape::{Record, schema_of};
use schemars::JsonSchema;
use schemars::generate::SchemaSettings;
use serde_json::{Map, Value, json};

use records::{
    ChangeRecord, ChangeV1, PlanRecord, PrototypeRecord, ReopenRecord, RunRecord, StopRecord,
    UnderstandingRecord,
};

/// The plugin's name, which its codes start with.
const PLUGIN: &str = "changes";
/// The severities that shut the review gate, which the review plugin reads
/// too.
pub const BLOCKING: [&str; 2] = ["critical", "high"];
/// The record type of a change, which the review plugin reads too.
pub const CHANGE_TYPE: &str = "changes/change/v2";
/// The record type of a change written before flows, which every reader of
/// a change still reads and `migrate` rewrites as a v2.
pub const CHANGE_V1: &str = "changes/change/v1";
/// Both record types of a change.
pub const CHANGE_TYPES: [&str; 2] = [CHANGE_V1, CHANGE_TYPE];

#[derive(Clone, Copy, Debug)]
pub struct Changes;

/// Something a capability says of what it wrote without refusing it.
#[derive(Debug, serde::Serialize, JsonSchema)]
pub(crate) struct Warning {
    pub(crate) code: &'static str,
    pub(crate) detail: String,
}

/// How long a capability that calls the tasks plugin may run, in seconds:
/// the nested call syncs and pushes a notes repo.
const NESTED_TIMEOUT: u64 = 120;
/// How long `sync` may run, in seconds: it runs gh once for each open
/// change with a pull request, each for up to 20 seconds.
const SYNC_TIMEOUT: u64 = 600;
/// The variable that sets how many days a change stays active under the
/// WIP limit.
const ACTIVE_DAYS_ENV: [&str; 1] = [wip::ACTIVE_DAYS_VARIABLE];
/// The variables gh reads for its account and configuration.
const GH_ENV: [&str; 3] = ["GH_TOKEN", "GITHUB_TOKEN", "GH_CONFIG_DIR"];

/// One capability: its name, what it does, the function that runs it, its
/// input and output schemas, its MCP hints (read-only, destructive,
/// idempotent, open-world), the variables it is given and how long it may
/// run.
struct Capability {
    name: &'static str,
    description: &'static str,
    run: fn(&dyn Host, &Value) -> Result<Value, PluginError>,
    input: fn() -> Value,
    output: fn() -> Value,
    hints: [bool; 4],
    env: &'static [&'static str],
    timeout: Option<u64>,
}

fn input<T: JsonSchema>() -> Value {
    schema_of::<T>(SchemaSettings::draft2020_12())
}

fn output<T: JsonSchema>() -> Value {
    schema_of::<T>(SchemaSettings::draft2020_12().for_serialize())
}

/// Every capability, in the order the manifest lists them. A capability
/// that rewrites a field of a change, or ends it, is destructive; one that
/// adds a task or reads GitHub reaches the open world.
const CAPABILITIES: [Capability; 19] = [
    Capability {
        name: "start",
        description: "Create a change in the first phase of its flow, its plan to fill in and, \
            unless one is named, its task; point the change at the stored version of its flow, \
            writing that version when the board lacks it; set its route from CODEOWNERS and \
            its check suite; record the open tasks, decisions and evals on the mount; and return \
            them with the repository's owners, contributing docs and obligations.",
        run: start::start,
        input: input::<start::Input>,
        output: output::<start::Output>,
        // A retry is refused once the change exists, but tasks.add may
        // already have added and pushed its task.
        hints: [false, false, false, true],
        env: &[],
        timeout: Some(NESTED_TIMEOUT),
    },
    Capability {
        name: "next",
        description: "The change's flow and phase, each check of its gate as pass or missing with \
            who can pass it and the remedy, whom it waits on, and what to do now.",
        run: phases::next,
        input: input::<phases::PathInput>,
        output: output::<phases::NextOutput>,
        hints: [true, false, true, false],
        env: &[],
        timeout: None,
    },
    Capability {
        name: "attach",
        description: "Add a reference to a record the change produced, which the plan the \
            change names keeps, or set its task, route, head commit, check suite, ticket or pull \
            request.",
        run: phases::attach,
        input: input::<phases::AttachInput>,
        output: output::<phases::AttachOutput>,
        hints: [false, true, true, false],
        env: &[],
        timeout: None,
    },
    Capability {
        name: "advance",
        description: "Move the change to the next phase of its flow when every check of its gate \
            that blocks passes; otherwise refuse and say what is missing and the remedy. Into build, refuse while three active changes of its kind \
            are in build for its mount, unless over_limit gives a reason, which the change \
            records and the output warns about.",
        run: phases::advance,
        input: input::<phases::AdvanceInput>,
        output: output::<phases::Moved>,
        hints: [false, true, false, false],
        env: &ACTIVE_DAYS_ENV,
        timeout: None,
    },
    Capability {
        name: "open",
        description: "The changes that are not done or dropped, with their flow, phase, route, task \
            and kind; by kind, every prototype with its question, state, verdict and branch; and the \
            day of the last sync.",
        run: open::open,
        input: input::<open::Input>,
        output: output::<open::Output>,
        hints: [true, false, true, false],
        env: &[],
        timeout: None,
    },
    Capability {
        name: "pr",
        description: "Write pr.md in the change's directory, a pull request body that says what \
            changes, why, what the tests prove and do not prove, and the evidence, and return the \
            gh command that would use it.",
        run: pr::pr,
        input: input::<phases::PathInput>,
        output: output::<pr::Output>,
        hints: [false, true, true, false],
        env: &[],
        timeout: None,
    },
    Capability {
        name: "plan",
        description: "Write a plan for the change, the records it uses and the paths it will \
            edit, given as JSON on stdin since uses, paths, considered and owed are lists, with \
            considered the surfaced records it does not build on and owed the obligations it \
            leaves owed, refusing a build change's plan without paths, a path outside the change's \
            mount, a reference that reaches nothing, a plan over 3,000 words and used documents \
            over 3,000 words in all; the new plan becomes the change's plan, and a plan equal to \
            the change's current one writes nothing and says unchanged.",
        run: plan::plan,
        input: input::<plan::Input>,
        output: output::<plan::Output>,
        // Each plan is a new record, and earlier plans stay.
        hints: [false, false, false, false],
        env: &[],
        timeout: None,
    },
    Capability {
        name: "brief",
        description: "What an agent needs to start: the change's plan, a one-line summary of each \
            control on its paths, one line per record it uses, the next step and the command \
            that lists the repository's controls, in at most 600 words.",
        run: brief::brief,
        input: input::<phases::PathInput>,
        output: output::<brief::Output>,
        hints: [true, false, true, false],
        env: &[],
        timeout: None,
    },
    Capability {
        name: "prototype",
        description: "Start a prototype that answers one question: add its task under a parent, \
            write its change in build on branch proto/<name>, and return its brief. It never \
            lands. Refused while three active prototypes explore for its mount, unless \
            over_limit gives a reason.",
        run: prototype::prototype,
        input: input::<prototype::Input>,
        output: output::<prototype::Output>,
        hints: [false, false, false, true],
        env: &ACTIVE_DAYS_ENV,
        timeout: Some(NESTED_TIMEOUT),
    },
    Capability {
        name: "verdict",
        description: "Record what a prototype found in one sentence, and the artifact version it \
            means, which ends it and its task: done when settled, dropped when abandoned.",
        run: prototype::verdict,
        input: input::<prototype::VerdictInput>,
        output: output::<prototype::VerdictOutput>,
        hints: [false, true, false, true],
        env: &[],
        timeout: Some(NESTED_TIMEOUT),
    },
    Capability {
        name: "pause",
        description: "Move a change in build to paused, which frees its place under the WIP \
            limit.",
        run: phases::pause,
        input: input::<phases::PathInput>,
        output: output::<phases::Moved>,
        hints: [false, true, false, false],
        env: &[],
        timeout: None,
    },
    Capability {
        name: "resume",
        description: "Move a paused change back to build, refused while three active changes of \
            its kind are in build for its mount, unless over_limit gives a reason.",
        run: phases::resume,
        input: input::<phases::AdvanceInput>,
        output: output::<phases::Moved>,
        hints: [false, true, false, false],
        env: &ACTIVE_DAYS_ENV,
        timeout: None,
    },
    Capability {
        name: "reopen",
        description: "Move a change back to the build phase of its flow for a fix round, such as \
            from review, and record why. Refused when its flow has no build phase before where \
            it stands, and while three active changes of its kind are in build for its mount, \
            unless over_limit gives a reason.",
        run: phases::reopen,
        input: input::<phases::ReopenInput>,
        output: output::<phases::Moved>,
        hints: [false, true, false, false],
        env: &ACTIVE_DAYS_ENV,
        timeout: None,
    },
    Capability {
        name: "reflow",
        description: "Move the change to the current version of the flow named, writing that \
            version when the board lacks it, refused when that flow has no phase where the change \
            stands.",
        run: phases::reflow,
        input: input::<phases::ReflowInput>,
        output: output::<phases::Reflowed>,
        hints: [false, true, true, false],
        env: &[],
        timeout: None,
    },
    Capability {
        name: "sync",
        description: "Read each open change's GitHub pull request with gh and end the change when \
            it closed: done, with its merge commit, when it merged and no attached task is open, \
            else dropped with the reason. List the changes with no pull request and what to do \
            with each. dry_run writes nothing; otherwise every move and the day of the sync are \
            written in one commit, or nothing when gh fails.",
        run: sync::sync,
        input: input::<sync::Input>,
        output: output::<sync::Output>,
        // A second sync against the same pull requests moves nothing more.
        hints: [false, true, true, true],
        env: &GH_ENV,
        timeout: Some(SYNC_TIMEOUT),
    },
    Capability {
        name: "drop",
        description: "End a change that is not done or dropped in dropped, recording the reason, \
            and name its task.",
        run: sync::drop,
        input: input::<sync::DropInput>,
        output: output::<sync::DropOutput>,
        hints: [false, true, false, false],
        env: &[],
        timeout: None,
    },
    Capability {
        name: "migrate",
        description: "Rewrite each changes/change/v1 as a changes/change/v2 that references the \
            stored version of the development flow, a prototype's with none; for one written before the split of \
            changes/change/v1, a prototype's fields move to changes/prototype/v1, the records a \
            change lists move to the plan it names, a pull request's URL becomes a github/pull/v1 \
            record through github.ref, and the sync stamp moves to Board's cache. flows also \
            replaces each change's own copy of its flow with a reference to the flow's stored \
            version, and rewrites each run that names its flow by name. plans moves each open \
            change's Considered and Obligations left owed sections from its plan's text to the \
            plan's considered and owed fields, in a new snapshot. apply writes every line \
            in one commit with a Board-Bulk trailer; without it nothing is written.",
        run: migrate::migrate,
        input: input::<migrate::Input>,
        output: output::<migrate::Output>,
        // Each pull request it records is written once; a second run finds
        // nothing to rewrite.
        hints: [false, true, true, true],
        env: &[],
        timeout: Some(SYNC_TIMEOUT),
    },
    Capability {
        name: "run",
        description: "Start a change, or resume one, set where its run stops (the flow's stop, \
            or through or until a phase) and build its run brief: each phase's instruction up to \
            the stop, the stop's rule, the surfaced records and the rules on its paths. inline \
            returns the brief and starts nothing; otherwise the agent BOARD_AGENT names, else \
            agent in the known boards file, runs on it in a worktree for the change, and run \
            records the run and why it stopped. sandbox starts the change in a throwaway copy \
            of the board and its repository, runs the agent there with its own home, and copies \
            the run and stop records back under the change it is a dry run for.",
        run: run::run,
        input: input::<run::Input>,
        output: output::<run::Output>,
        hints: [false, true, false, true],
        env: &run::ENV,
        timeout: Some(run::TIMEOUT),
    },
    Capability {
        name: "stop",
        description: "Record why a run of the change stopped: reached, waiting, refused, budget \
            or error, with the phase, the blocking checks missing there, the question it left \
            for the owner and a one-line summary.",
        run: stop::stop,
        input: input::<stop::Input>,
        output: output::<stop::Output>,
        // Each stop is a new record.
        hints: [false, false, false, false],
        env: &[],
        timeout: None,
    },
];

impl Plugin for Changes {
    fn manifest(&self) -> Manifest {
        Manifest::built_in(manifest()).expect(
            "the manifest is built from constants and its inputs' schemas; a test parses it",
        )
    }

    fn schemas(&self) -> Vec<(RecordType, Value)> {
        let settings = SchemaSettings::draft2020_12;
        let typed = |name: &str, schema: Value| {
            (
                name.parse()
                    .expect("TYPE is a record type; a test parses it"),
                schema,
            )
        };
        vec![
            typed(ChangeRecord::TYPE, schema_of::<ChangeRecord>(settings())),
            typed(ChangeV1::TYPE, schema_of::<ChangeV1>(settings())),
            typed(PlanRecord::TYPE, schema_of::<PlanRecord>(settings())),
            typed(
                UnderstandingRecord::TYPE,
                schema_of::<UnderstandingRecord>(settings()),
            ),
            typed(
                PrototypeRecord::TYPE,
                schema_of::<PrototypeRecord>(settings()),
            ),
            typed(flow::Flow::TYPE, schema_of::<flow::Flow>(settings())),
            typed(RunRecord::TYPE, schema_of::<RunRecord>(settings())),
            typed(StopRecord::TYPE, schema_of::<StopRecord>(settings())),
            typed(ReopenRecord::TYPE, schema_of::<ReopenRecord>(settings())),
        ]
    }

    fn call(
        &self,
        host: &dyn Host,
        capability: &CapabilityName,
        input: Value,
    ) -> Result<Value, PluginError> {
        let found = (CAPABILITIES.iter()).find(|offered| offered.name == capability.as_str());
        let Some(offered) = found else {
            let names: Vec<&str> = CAPABILITIES.iter().map(|offered| offered.name).collect();
            return Err(refusal(
                "changes::call::unknown_capability",
                format!(
                    "changes offers no capability {capability}; it offers {}",
                    names.join(", ")
                ),
                [],
            ));
        };
        (offered.run)(host, &input)
    }
}

fn manifest() -> Value {
    let capabilities: Vec<Value> = (CAPABILITIES.iter())
        .map(|offered| {
            let [read_only, destructive, idempotent, open_world] = offered.hints;
            let mut tool = json!({
                "name": offered.name,
                "description": offered.description,
                "inputSchema": (offered.input)(),
                "outputSchema": (offered.output)(),
                "annotations": {
                    "readOnlyHint": read_only,
                    "destructiveHint": destructive,
                    "idempotentHint": idempotent,
                    "openWorldHint": open_world,
                },
                "_meta": {},
            });
            if !offered.env.is_empty() {
                tool["env"] = json!(offered.env);
            }
            if let Some(timeout) = offered.timeout {
                tool["timeout"] = json!(timeout);
            }
            tool
        })
        .collect();
    json!({
        "$type": "board/plugin-manifest/v1",
        "name": PLUGIN,
        "version": "0.1.0",
        // SKILL.md's description, the callout an agent sees first.
        "description": "Walk one change to a repository from its plan to its pull request, and \
            gather every record it produces. Use it to start, resume or list work on a \
            repository the board mounts. Start a change with `board -C <board> \
            changes start --mount /repos/<name> --title '<title>'`, then run `board changes \
            next --path /changes/<name>` for the next step. `board changes open` lists the \
            changes in progress. SKILL.md and README.md in the plugin's \
            directory say more.",
        "metadata": {"area": "delivery"},
        "types": [
            ChangeV1::TYPE,
            ChangeRecord::TYPE,
            flow::Flow::TYPE,
            PlanRecord::TYPE,
            PrototypeRecord::TYPE,
            ReopenRecord::TYPE,
            RunRecord::TYPE,
            StopRecord::TYPE,
            UnderstandingRecord::TYPE,
        ],
        "capabilities": capabilities,
    })
}

/// A refusal of the plugin's own, with `fields`.
pub(crate) fn refusal<const N: usize>(
    code: &str,
    message: String,
    fields: [(&str, Value); N],
) -> PluginError {
    PluginError {
        code: code.to_owned(),
        message,
        fields: (fields.into_iter())
            .map(|(name, value)| (name.to_owned(), value))
            .collect::<Map<_, _>>(),
        hint: None,
    }
}

/// `value` as JSON, for an output built from strings, numbers and lists.
pub(crate) fn to_value(value: impl serde::Serialize) -> Value {
    serde_json::to_value(value).expect("the output is plain strings, numbers and lists")
}

#[cfg(test)]
mod tests;
