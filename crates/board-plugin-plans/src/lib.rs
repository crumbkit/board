//! The plans plugin built into Board (plugins/plans/README.md).

mod answer;
mod open;
mod question;
pub mod records;
mod withdraw;

use board_model::{CapabilityName, RecordType};
use board_ops::{Host, Plugin, PluginError};
use board_plugin::Manifest;
use board_plugin::shape::{Record, schema_of};
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
            other => Err(PluginError {
                code: "plans::call::unknown_capability".to_owned(),
                message: format!(
                    "plans offers no capability {other}; it offers open, answer and withdraw"
                ),
                fields: Map::new(),
                hint: None,
            }),
        }
    }
}

fn manifest() -> Value {
    json!({
        "$type": "board/plugin-manifest/v1",
        "name": "plans",
        "version": "0.1.0",
        "description": "The plans plugin records the decisions a session makes, the questions only \
            the owner can answer, and the owner's answers, as records in the board. Use it when \
            you choose between options that a later session or reviewer needs to know about, and \
            when work waits on the owner. Write a decision with \
            `board -C <board> put --new --var project=<project>` and a `plans/decision/v1` on \
            stdin; `board -C <board> plans open` lists the questions still waiting for an \
            answer. SKILL.md and README.md in the plugin's directory say more.",
        "metadata": {"area": "planning"},
        "types": record_schemas().map(|(name, _)| name),
        "capabilities": [{
            "name": "open",
            "description": "The questions under a path whose status is open and that no answer names, \
                oldest first.",
            "inputSchema": schema_of::<open::Input>(SchemaSettings::draft2020_12()),
            "outputSchema": schema_of::<open::Output>(SchemaSettings::draft2020_12().for_serialize()),
            "annotations": {
                "readOnlyHint": true,
                "destructiveHint": false,
                "idempotentHint": true,
                "openWorldHint": false,
            },
            "_meta": {},
        }, {
            "name": "answer",
            "description": "Record the owner's answer to an open question. The answer is what \
                marks the question answered; the question's content is left unchanged.",
            "inputSchema": schema_of::<answer::Input>(SchemaSettings::draft2020_12()),
            "outputSchema": schema_of::<answer::Output>(SchemaSettings::draft2020_12().for_serialize()),
            "annotations": {
                "readOnlyHint": false,
                "destructiveHint": false,
                "idempotentHint": true,
                "openWorldHint": false,
            },
            "_meta": {},
        }, {
            "name": "withdraw",
            "description": "Withdraw an open question that no answer names, with the reason, so \
                that plans.open stops listing it.",
            "inputSchema": schema_of::<withdraw::Input>(SchemaSettings::draft2020_12()),
            "outputSchema": schema_of::<withdraw::Output>(SchemaSettings::draft2020_12().for_serialize()),
            "annotations": {
                "readOnlyHint": false,
                "destructiveHint": true,
                "idempotentHint": true,
                "openWorldHint": false,
            },
            "_meta": {},
        }],
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
