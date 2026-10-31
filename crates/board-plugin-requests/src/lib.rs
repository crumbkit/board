//! The requests plugin built into Board (docs/plugins/built-in/requests.md).

pub mod records;
mod thread;

use board_model::{CapabilityName, RecordType};
use board_ops::{Host, Plugin, PluginError};
use board_plugin::Manifest;
use board_plugin_plans::shape::{Record, schema_of};
use schemars::generate::SchemaSettings;
use serde_json::{Map, Value, json};

use crate::records::ReplyRecord;

#[derive(Clone, Copy, Debug)]
pub struct Requests;

impl Plugin for Requests {
    fn manifest(&self) -> Manifest {
        Manifest::built_in(manifest())
            .expect("the manifest is built from constants and its type's schema; a test parses it")
    }

    fn schemas(&self) -> Vec<(RecordType, Value)> {
        let name = (ReplyRecord::TYPE.parse())
            .expect("the reply's TYPE is a record type; a test parses it");
        vec![(name, reply_schema())]
    }

    fn call(
        &self,
        host: &dyn Host,
        capability: &CapabilityName,
        input: Value,
    ) -> Result<Value, PluginError> {
        match capability.as_str() {
            "thread" => thread::thread(host, &input),
            other => Err(PluginError {
                code: "requests::call::unknown_capability".to_owned(),
                message: format!("requests offers no capability {other}; it offers thread"),
                fields: Map::new(),
            }),
        }
    }
}

fn manifest() -> Value {
    json!({
        "$type": "board/plugin-manifest/v1",
        "name": "requests",
        "version": "0.1.0",
        "description": "A plugin's request to the owner as a thread: every version of a pending effect, \
            the owner's answers and the owner's replies. Run `board --personal call requests.thread \
            --effect <id>` with any version's id. docs/plugins/built-in/requests.md in Board's repository says \
            where a reply lives.",
        "metadata": {"area": "effects"},
        "types": [ReplyRecord::TYPE],
        "capabilities": [{
            "name": "thread",
            "description": "Every version of the effect's request, oldest first, with its answer, \
                and the owner's replies to them, oldest first.",
            "inputSchema": schema_of::<thread::Input>(SchemaSettings::draft2020_12()),
            "outputSchema": schema_of::<thread::Output>(SchemaSettings::draft2020_12().for_serialize()),
            "annotations": {"readOnlyHint": true},
            "_meta": {},
        }],
    })
}

/// The schema `ReplyRecord` generates.
fn reply_schema() -> Value {
    schema_of::<ReplyRecord>(SchemaSettings::draft2020_12())
}

#[cfg(test)]
mod tests;
