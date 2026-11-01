//! The controls plugin built into Board (plugins/controls/README.md).

mod check;
mod evaluate;
mod for_path;
mod index;
mod judged;
pub mod records;

use board_model::{CapabilityName, Code, Fields, LogicalPath, RecordType, Refusal};
use board_ops::{Host, Plugin, PluginError};
use board_plugin::Manifest;
use board_plugin::shape::{Record, schema_of};
use schemars::generate::SchemaSettings;
use serde_json::{Map, Value, json};

use records::ControlRecord;

#[derive(Clone, Copy, Debug)]
pub struct Controls;

impl Plugin for Controls {
    fn manifest(&self) -> Manifest {
        Manifest::built_in(manifest()).expect(
            "the manifest is built from constants and its inputs' schemas; a test parses it",
        )
    }

    fn schemas(&self) -> Vec<(RecordType, Value)> {
        let name = (ControlRecord::TYPE.parse())
            .expect("ControlRecord's TYPE is a record type; a test parses it");
        vec![(name, record_schema())]
    }

    fn call(
        &self,
        host: &dyn Host,
        capability: &CapabilityName,
        input: Value,
    ) -> Result<Value, PluginError> {
        match capability.as_str() {
            "for-path" => for_path::for_path(host, &input),
            "check" => check::check(host, &input),
            "evaluate" => evaluate::evaluate(host, &input),
            other => Err(PluginError {
                code: "controls::call::unknown_capability".to_owned(),
                message: format!(
                    "controls offers no capability {other}; it offers for-path, check and evaluate"
                ),
                fields: Map::new(),
            }),
        }
    }
}

/// The schema of `controls/control/v1`.
fn record_schema() -> Value {
    schema_of::<ControlRecord>(SchemaSettings::draft2020_12())
}

/// `path` as a logical path, or the refusal `board refs` gives a path it
/// cannot read, as the Python plugin reported it.
fn logical(path: &str) -> Result<LogicalPath, PluginError> {
    path.parse().map_err(|error| {
        let refusal = Refusal::new(Code::CliUsage, error, &Fields::default());
        PluginError::board_refused("controls", "refs", &refusal)
    })
}

fn manifest() -> Value {
    let settings = || SchemaSettings::draft2020_12().for_serialize();
    json!({
        "$type": "board/plugin-manifest/v1",
        "name": "controls",
        "version": "0.1.0",
        "description": "The controls plugin keeps requirements on files as records in the board that \
            holds the files. Use it when you start work in a board or a repository it mounts, and before \
            you edit a file a control may govern. Start with `board -C <board> call controls.check --path \
            /<directory>`, which lists the budgets it measures and the written requirements under that \
            path. `board -C <board> call controls.for-path --path /<file>` lists the controls on one file. \
            `board -C <board> call controls.evaluate` checks text, such as a draft, against the rules of \
            the controls it names before the text is written. SKILL.md and README.md in the plugin's directory say more.",
        "metadata": {"area": "governance"},
        "types": [ControlRecord::TYPE],
        "capabilities": [{
            "name": "for-path",
            "description": "The controls that apply to a path.",
            "inputSchema": schema_of::<for_path::Input>(SchemaSettings::draft2020_12()),
            "outputSchema": schema_of::<for_path::Output>(settings()),
            "annotations": {"readOnlyHint": true},
            "_meta": {},
        }, {
            "name": "check",
            "description": "Check the controls under a path, `/` when left out. Fails naming each \
                control over budget or degraded.",
            "inputSchema": schema_of::<check::Input>(SchemaSettings::draft2020_12()),
            "outputSchema": schema_of::<check::Output>(settings()),
            "annotations": {"readOnlyHint": true},
            "_meta": {},
        }, {
            "name": "evaluate",
            "description": "Check named parts of text against controls' budget, pattern and judged \
                rules, and return each control's verdict on each part with its findings.",
            "inputSchema": schema_of::<evaluate::Input>(SchemaSettings::draft2020_12()),
            "outputSchema": schema_of::<evaluate::Output>(settings()),
            "annotations": {"readOnlyHint": true},
            "_meta": {},
            "timeout": judged::EVALUATE_TIMEOUT,
            "env": [judged::AGENT_VARIABLE, "ANTHROPIC_API_KEY", "CLAUDE_CODE_OAUTH_TOKEN"],
        }],
    })
}

#[cfg(test)]
mod tests;
