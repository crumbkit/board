//! The runs plugin built into Board (plugins/runs/README.md).

mod latest;
mod qualifying;
pub mod records;
mod time;
mod trend;

use board_model::{CapabilityName, RecordType};
use board_ops::{Host, Plugin, PluginError};
use board_plugin::Manifest;
use board_plugin::shape::{Record, schema_of};
use schemars::generate::SchemaSettings;
use serde_json::{Map, Value, json};

use records::{RunRecord, SuiteRecord};

#[derive(Clone, Copy, Debug)]
pub struct Runs;

impl Plugin for Runs {
    fn manifest(&self) -> Manifest {
        Manifest::built_in(manifest()).expect(
            "the manifest is built from constants and its inputs' schemas; a test parses it",
        )
    }

    fn schemas(&self) -> Vec<(RecordType, Value)> {
        let settings = SchemaSettings::draft2020_12;
        vec![
            (
                SuiteRecord::TYPE
                    .parse()
                    .expect("SuiteRecord's TYPE is a record type; a test parses it"),
                schema_of::<SuiteRecord>(settings()),
            ),
            (
                RunRecord::TYPE
                    .parse()
                    .expect("RunRecord's TYPE is a record type; a test parses it"),
                schema_of::<RunRecord>(settings()),
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
            "latest" => latest::latest(host, &input),
            "trend" => trend::trend(host, &input),
            other => Err(PluginError {
                code: "runs::call::unknown_capability".to_owned(),
                message: format!("runs offers no capability {other}; it offers latest and trend"),
                fields: Map::new(),
                hint: None,
            }),
        }
    }
}

fn manifest() -> Value {
    let output = || SchemaSettings::draft2020_12().for_serialize();
    json!({
        "$type": "board/plugin-manifest/v1",
        "name": "runs",
        "version": "0.1.0",
        "description": "Suites of tests, benchmarks and evals, and the runs that record what each gave. \
            `board call runs.latest --suite /suites/<name>` returns the latest full run of a clean revision; \
            `board call runs.trend --suite /suites/<name> --measure <name>` returns a measure over such runs. \
            README.md in the plugin's directory says how to record a run.",
        "metadata": {"area": "testing"},
        "types": [SuiteRecord::TYPE, RunRecord::TYPE],
        "capabilities": [{
            "name": "latest",
            "description": "The latest full run of a clean revision of a suite, with its outcome, measures, \
                failed cases and age, or null when no run qualifies.",
            "inputSchema": schema_of::<latest::Input>(SchemaSettings::draft2020_12()),
            "outputSchema": schema_of::<latest::Output>(output()),
            "annotations": {"readOnlyHint": true},
            "_meta": {},
        }, {
            "name": "trend",
            "description": "One measure of a suite, or of one of its cases, over its full runs of clean \
                revisions, newest first.",
            "inputSchema": schema_of::<trend::Input>(SchemaSettings::draft2020_12()),
            "outputSchema": schema_of::<trend::Output>(output()),
            "annotations": {"readOnlyHint": true},
            "_meta": {},
        }],
    })
}

#[cfg(test)]
mod tests;
