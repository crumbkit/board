//! The github plugin built into Board (plugins/github/README.md). Each
//! capability is one module, and every call to GitHub goes through `gh`.

mod gh;
mod objects;
pub mod records;
mod reference;

use board_model::{CapabilityName, RecordType};
use board_ops::{Host, Plugin, PluginError};
use board_plugin::Manifest;
use board_plugin::shape::{Record, schema_of};
use schemars::generate::SchemaSettings;
use serde_json::{Map, Value, json};

use records::{IssueRecord, PullRecord};

#[derive(Clone, Copy, Debug)]
pub struct Github;

impl Plugin for Github {
    fn manifest(&self) -> Manifest {
        Manifest::built_in(manifest()).expect(
            "the manifest is built from constants and its inputs' schemas; a test parses it",
        )
    }

    fn schemas(&self) -> Vec<(RecordType, Value)> {
        let settings = SchemaSettings::draft2020_12;
        vec![
            (
                PullRecord::TYPE
                    .parse()
                    .expect("PullRecord's TYPE is a record type; a test parses it"),
                schema_of::<PullRecord>(settings()),
            ),
            (
                IssueRecord::TYPE
                    .parse()
                    .expect("IssueRecord's TYPE is a record type; a test parses it"),
                schema_of::<IssueRecord>(settings()),
            ),
        ]
    }

    fn call(
        &self,
        host: &dyn Host,
        capability: &CapabilityName,
        input: Value,
    ) -> Result<Value, PluginError> {
        match capability.as_str() {
            "ref" => reference::reference(host, &input),
            other => Err(PluginError {
                code: "github::call::unknown_capability".to_owned(),
                message: format!("github offers no capability {other}; it offers ref"),
                fields: Map::new(),
            }),
        }
    }
}

fn manifest() -> Value {
    json!({
        "$type": "board/plugin-manifest/v1",
        "name": "github",
        "version": "0.1.0",
        // SKILL.md's description, the callout an agent sees first.
        "description": "Record a GitHub pull request or issue as a typed record that other records \
            can reference. Use it when a record should name a pull request or issue. Run \
            `board -C <board> call github.ref --ref <owner>/<repo>#<number>`, then reference the \
            `path` it returns. SKILL.md and README.md in the plugin's directory say more.",
        "metadata": {"area": "delivery"},
        "types": [PullRecord::TYPE, IssueRecord::TYPE],
        "capabilities": [{
            "name": "ref",
            "description": "Write the record of a pull request or issue, asking gh which it is \
                and its title, or report the record already there without asking.",
            "inputSchema": schema_of::<reference::Input>(SchemaSettings::draft2020_12()),
            "outputSchema": schema_of::<reference::Output>(SchemaSettings::draft2020_12().for_serialize()),
            "annotations": {"readOnlyHint": false},
            "env": gh::ENV,
            "_meta": {},
        }],
    })
}

#[cfg(test)]
mod tests;
