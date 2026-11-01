//! The jira plugin built into Board (plugins/jira/README.md).

mod issue;
pub mod records;

use board_model::{CapabilityName, RecordType};
use board_ops::{Host, Plugin, PluginError};
use board_plugin::Manifest;
use board_plugin::shape::{Record, schema_of};
use schemars::generate::SchemaSettings;
use serde_json::{Map, Value, json};

use records::IssueRecord;

#[derive(Clone, Copy, Debug)]
pub struct Jira;

impl Plugin for Jira {
    fn manifest(&self) -> Manifest {
        Manifest::built_in(manifest()).expect(
            "the manifest is built from constants and its inputs' schemas; a test parses it",
        )
    }

    fn schemas(&self) -> Vec<(RecordType, Value)> {
        let mut schema = schema_of::<IssueRecord>(SchemaSettings::draft2020_12());
        // The Python plugin's schema had no title, and keeping its bytes
        // spares a board that enabled it a `plugin enable`.
        if let Some(schema) = schema.as_object_mut() {
            schema.remove("title");
        }
        vec![(
            IssueRecord::TYPE
                .parse()
                .expect("IssueRecord's TYPE is a record type; a test parses it"),
            schema,
        )]
    }

    fn call(
        &self,
        host: &dyn Host,
        capability: &CapabilityName,
        input: Value,
    ) -> Result<Value, PluginError> {
        match capability.as_str() {
            "ref" => issue::reference(host, &input),
            other => Err(PluginError {
                code: "jira::call::unknown_capability".to_owned(),
                message: format!("jira offers no capability {other}; it offers ref"),
                fields: Map::new(),
            }),
        }
    }
}

fn manifest() -> Value {
    json!({
        "$type": "board/plugin-manifest/v1",
        "name": "jira",
        "version": "0.1.0",
        // SKILL.md's description, the callout an agent sees first.
        "description": "Record a Jira issue as a typed jira/issue/v1 record that other records can \
            reference by path. Use it when work names a Jira ticket. Start with \
            `board -C <board> call jira.ref --key ACME-123 --site acme`, which writes \
            /jira/acme/ACME-123 once with its title. SKILL.md and README.md in the plugin's \
            directory say more.",
        "metadata": {"area": "planning"},
        "types": [IssueRecord::TYPE],
        "capabilities": [{
            "name": "ref",
            "description": "Write the issue at /jira/<site>/<key> with its title, read once \
                through acli, unless a record is already there.",
            "inputSchema": schema_of::<issue::Input>(SchemaSettings::draft2020_12()),
            "outputSchema": schema_of::<issue::Output>(SchemaSettings::draft2020_12().for_serialize()),
            "annotations": {"idempotentHint": true},
            "_meta": {},
        }],
    })
}

#[cfg(test)]
mod tests;
