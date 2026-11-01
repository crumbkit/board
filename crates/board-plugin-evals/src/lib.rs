//! The evals plugin built into Board (plugins/evals/README.md).

mod list;
pub mod records;

use board_model::{CapabilityName, RecordType};
use board_ops::{Host, Plugin, PluginError};
use board_plugin::Manifest;
use board_plugin::shape::{Record, schema_of};
use schemars::generate::SchemaSettings;
use serde_json::{Map, Value, json};

use records::EvalRecord;

#[derive(Clone, Copy, Debug)]
pub struct Evals;

impl Plugin for Evals {
    fn manifest(&self) -> Manifest {
        Manifest::built_in(manifest()).expect(
            "the manifest is built from constants and its inputs' schemas; a test parses it",
        )
    }

    fn schemas(&self) -> Vec<(RecordType, Value)> {
        let name = (EvalRecord::TYPE.parse())
            .expect("EvalRecord's TYPE is a record type; a test parses it");
        vec![(
            name,
            schema_of::<EvalRecord>(SchemaSettings::draft2020_12()),
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
                code: "evals::call::unknown_capability".to_owned(),
                message: format!("evals offers no capability {other}; it offers about and open"),
                fields: Map::new(),
                hint: None,
            }),
        }
    }
}

fn manifest() -> Value {
    let output = || schema_of::<list::Output>(SchemaSettings::draft2020_12().for_serialize());
    json!({
        "$type": "board/plugin-manifest/v1",
        "name": "evals",
        "version": "0.1.0",
        "description": "Record every test of a library, tool, model or agent as an eval and its runs in the \
            board, and check what was already tested before testing. Use it before you start a test and \
            when you conclude one. Start with `board -C <board> call evals.about --subject <name>`, which \
            lists the evals of what you will test. `board -C <board> call evals.open` lists those still \
            planned or running. SKILL.md and README.md in the plugin's directory say more.",
        "metadata": {"area": "testing"},
        "types": [EvalRecord::TYPE],
        "capabilities": [{
            "name": "about",
            "description": "The evals that name a subject, or that name a repository's mount or live in it, \
                whatever their status, here and in mounted boards.",
            "inputSchema": schema_of::<list::AboutInput>(SchemaSettings::draft2020_12()),
            "outputSchema": output(),
            "annotations": {"readOnlyHint": true},
            "_meta": {},
        }, {
            "name": "open",
            "description": "The evals still planned or running, here and in mounted boards.",
            "inputSchema": schema_of::<list::OpenInput>(SchemaSettings::draft2020_12()),
            "outputSchema": output(),
            "annotations": {"readOnlyHint": true},
            "_meta": {},
        }],
    })
}

#[cfg(test)]
mod tests;
