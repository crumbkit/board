//! The decisions plugin built into Board (plugins/decisions/README.md).

mod list;
pub mod records;
pub mod standing;

use board_model::{CapabilityName, RecordType};
use board_ops::{Host, Plugin, PluginError};
use board_plugin::Manifest;
use board_plugin::shape::{Record, schema_of};
use schemars::generate::SchemaSettings;
use serde_json::{Map, Value, json};

use records::DecisionRecord;

#[derive(Clone, Copy, Debug)]
pub struct Decisions;

impl Plugin for Decisions {
    fn manifest(&self) -> Manifest {
        Manifest::built_in(manifest()).expect(
            "the manifest is built from constants and its inputs' schemas; a test parses it",
        )
    }

    fn schemas(&self) -> Vec<(RecordType, Value)> {
        let name = (DecisionRecord::TYPE.parse())
            .expect("DecisionRecord's TYPE is a record type; a test parses it");
        vec![(
            name,
            schema_of::<DecisionRecord>(SchemaSettings::draft2020_12()),
        )]
    }

    fn call(
        &self,
        host: &dyn Host,
        capability: &CapabilityName,
        input: Value,
    ) -> Result<Value, PluginError> {
        match capability.as_str() {
            "about" => list::about(host, &input),
            "open" => list::open(host, &input),
            other => Err(PluginError {
                code: "decisions::call::unknown_capability".to_owned(),
                message: format!(
                    "decisions offers no capability {other}; it offers about and open"
                ),
                fields: Map::new(),
            }),
        }
    }
}

fn manifest() -> Value {
    let output = || schema_of::<list::Output>(SchemaSettings::draft2020_12().for_serialize());
    json!({
        "$type": "board/plugin-manifest/v1",
        "name": "decisions",
        "version": "0.1.0",
        "description": "Decisions sessions record, each a choice and why, written once. \
            `board call decisions.open` lists those awaiting the owner's review, oldest first, and \
            `board call decisions.about --ref <path>` lists those whose basis names a path. \
            plugins/decisions/README.md in Board's repository says how to record and review one.",
        "metadata": {"area": "planning"},
        "types": [DecisionRecord::TYPE],
        "capabilities": [{
            "name": "about",
            "description": "The decisions not superseded whose basis names a path or an entry under it, \
                newest first, here and in mounted boards.",
            "inputSchema": schema_of::<list::AboutInput>(SchemaSettings::draft2020_12()),
            "outputSchema": output(),
            "annotations": {"readOnlyHint": true},
            "_meta": {},
        }, {
            "name": "open",
            "description": "The decisions under a path that an open question asks the owner to review, \
                oldest first.",
            "inputSchema": schema_of::<list::OpenInput>(SchemaSettings::draft2020_12()),
            "outputSchema": output(),
            "annotations": {"readOnlyHint": true},
            "_meta": {},
        }],
    })
}

#[cfg(test)]
mod tests;
