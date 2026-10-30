//! The plans plugin built into Board (plugins/plans/README.md).

mod open;

use board_model::{CapabilityName, RecordType, parse_json};
use board_ops::{Host, Plugin, PluginError};
use board_plugin::Manifest;
use schemars::generate::SchemaSettings;
use serde_json::{Map, Value, json};

/// Each type the plugin ships, with its schema. No Rust code reads these
/// records, so their schemas are kept as files rather than generated.
const TYPES: [(&str, &str); 3] = [
    (
        "plans/question/v1",
        include_str!("../types/plans/question/v1.schema.json"),
    ),
    (
        "plans/answer/v1",
        include_str!("../types/plans/answer/v1.schema.json"),
    ),
    (
        "plans/decision/v1",
        include_str!("../types/plans/decision/v1.schema.json"),
    ),
];

#[derive(Clone, Copy, Debug)]
pub struct Plans;

impl Plugin for Plans {
    fn manifest(&self) -> Manifest {
        Manifest::built_in(manifest())
            .expect("the manifest is built from constants and its types' schemas; a test parses it")
    }

    fn schemas(&self) -> Vec<(RecordType, Value)> {
        (TYPES.iter())
            .filter_map(|(name, text)| {
                Some((name.parse().ok()?, parse_json(text.as_bytes()).ok()?))
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
        "types": TYPES.map(|(name, _)| name),
        "capabilities": [{
            "name": "open",
            "description": "The questions under a path whose status is open, oldest first.",
            "inputSchema": open::schema::<open::Input>(SchemaSettings::draft2020_12()),
            "outputSchema": open::schema::<open::Output>(SchemaSettings::draft2020_12().for_serialize()),
            "annotations": {"readOnlyHint": true},
            "_meta": {},
        }],
    })
}

#[cfg(test)]
mod tests;
