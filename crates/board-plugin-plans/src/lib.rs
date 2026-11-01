//! The plans plugin built into Board (plugins/plans/README.md).

mod open;
pub mod records;

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
            other => Err(PluginError {
                code: "plans::call::unknown_capability".to_owned(),
                message: format!("plans offers no capability {other}; it offers open"),
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
        "description": "Questions an agent session asks the owner, the owner's answers, and the decisions \
            sessions record. `board call plans.open` lists the questions still waiting for an answer, \
            oldest first. plugins/plans/README.md in Board's repository says where each record lives.",
        "metadata": {"area": "planning"},
        "types": record_schemas().map(|(name, _)| name),
        "capabilities": [{
            "name": "open",
            "description": "The questions under a path whose status is open, oldest first.",
            "inputSchema": schema_of::<open::Input>(SchemaSettings::draft2020_12()),
            "outputSchema": schema_of::<open::Output>(SchemaSettings::draft2020_12().for_serialize()),
            "annotations": {"readOnlyHint": true},
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
