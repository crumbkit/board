//! The inflight plugin built into Board (plugins/inflight/README.md).

mod now;

use board_model::{CapabilityName, RecordType};
use board_ops::{Host, Plugin, PluginError};
use board_plugin::Manifest;
use board_plugin::shape::schema_of;
use schemars::generate::SchemaSettings;
use serde_json::{Map, Value, json};

/// How long `now` may take, in seconds. It calls `changes.next` once per
/// change it lists, and while changes is a Python plugin each call starts a
/// process that takes up to about a second on a large board.
const NOW_TIMEOUT: u64 = 120;

#[derive(Clone, Copy, Debug)]
pub struct Inflight;

impl Plugin for Inflight {
    fn manifest(&self) -> Manifest {
        Manifest::built_in(manifest())
            .expect("the manifest is built from constants and its input's schema; a test parses it")
    }

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
            "now" => now::now(host, &input),
            other => Err(PluginError {
                code: "inflight::call::unknown_capability".to_owned(),
                message: format!("inflight offers no capability {other}; it offers now"),
                fields: Map::new(),
                hint: None,
            }),
        }
    }
}

fn manifest() -> Value {
    json!({
        "$type": "board/plugin-manifest/v1",
        "name": "inflight",
        "version": "0.1.0",
        "description": "What is open right now: each open change with its phase, gate, next instruction, \
            pull request and artifacts, then the questions waiting on the owner and the open evals. Use \
            it to see what is in flight, as after a usage stop, with \
            `board --personal call inflight.now`. README.md in the plugin's directory says more.",
        "metadata": {"area": "planning"},
        "types": [],
        "capabilities": [{
            "name": "now",
            "description": "List the open changes with their phase, gate, instruction, pull request and \
                artifacts, the open questions and the open evals, each part bounded.",
            "inputSchema": schema_of::<now::Input>(SchemaSettings::draft2020_12()),
            "outputSchema": schema_of::<now::Output>(SchemaSettings::draft2020_12().for_serialize()),
            "annotations": {"readOnlyHint": true},
            "_meta": {},
            "timeout": NOW_TIMEOUT,
        }],
    })
}

#[cfg(test)]
mod tests;
