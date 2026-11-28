//! The share plugin built into Board (plugins/share/README.md).

mod closure;
mod to;

use board_model::{CapabilityName, RecordType};
use board_ops::{Host, Plugin, PluginError};
use board_plugin::Manifest;
use board_plugin::shape::schema_of;
use schemars::generate::SchemaSettings;
use serde_json::{Map, Value, json};

#[derive(Clone, Copy, Debug)]
pub struct Share;

impl Plugin for Share {
    fn manifest(&self) -> Manifest {
        Manifest::built_in(manifest()).expect(
            "the manifest is built from constants and its inputs' schemas; a test parses it",
        )
    }

    /// The plugin ships no types.
    fn schemas(&self) -> Vec<(RecordType, Value)> {
        Vec::new()
    }

    fn call(
        &self,
        host: &dyn Host,
        capability: &CapabilityName,
        input: Value,
    ) -> Result<Value, PluginError> {
        match capability.as_str() {
            "to" => to::share_to(host, &input),
            other => Err(PluginError {
                code: "share::call::unknown_capability".to_owned(),
                message: format!("share offers no capability {other}; it offers to"),
                fields: Map::new(),
                hint: None,
            }),
        }
    }
}

fn manifest() -> Value {
    json!({
        "$type": "board/plugin-manifest/v1",
        "name": "share",
        "version": "0.1.0",
        // SKILL.md's description, the callout an agent sees first.
        "description": "Copies a record into a mounted board, such as a team's, with what it \
            references and the schemas of its types, so a reader there can open it. Use it when \
            someone else needs to read your record.",
        "metadata": {"area": "sharing"},
        "capabilities": [{
            "name": "to",
            "description": "Copy a record, its children, what they reference or link to, the records \
                that reference it or its children, the decisions and questions of what it is part of, and the schemas it lacks into a mounted board, \
                as one commit there, and move the mount's pin to it; without `yes`, list them and \
                write nothing.",
            "inputSchema": schema_of::<to::Input>(SchemaSettings::draft2020_12()),
            "outputSchema": schema_of::<to::Output>(SchemaSettings::draft2020_12().for_serialize()),
            "annotations": {
                "readOnlyHint": false,
                "destructiveHint": false,
                "idempotentHint": true,
                "openWorldHint": false,
            },
            "_meta": {},
        }],
    })
}

#[cfg(test)]
mod tests;
