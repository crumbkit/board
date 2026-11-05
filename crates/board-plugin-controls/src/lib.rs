//! The controls plugin built into Board (plugins/controls/README.md).

mod check;
mod evaluate;
mod for_path;
mod for_paths;
mod index;
mod judged;
mod overlap;
pub mod records;
mod standards;
mod summary;

use board_model::{CapabilityName, Code, Fields, LogicalPath, RecordType, Refusal};
use board_ops::{Host, Plugin, PluginError};
use board_plugin::Manifest;
use board_plugin::shape::{Record, schema_of};
use schemars::generate::SchemaSettings;
use serde_json::{Map, Value, json};

use records::{ControlRecord, SourceRecord};

#[derive(Clone, Copy, Debug)]
pub struct Controls;

impl Plugin for Controls {
    fn manifest(&self) -> Manifest {
        Manifest::built_in(manifest()).expect(
            "the manifest is built from constants and its inputs' schemas; a test parses it",
        )
    }

    fn schemas(&self) -> Vec<(RecordType, Value)> {
        let schema = |name: &str, schema: Value| {
            let name =
                (name.parse()).expect("each record's TYPE is a record type; a test parses it");
            (name, schema)
        };
        let settings = SchemaSettings::draft2020_12;
        vec![
            schema(ControlRecord::TYPE, schema_of::<ControlRecord>(settings())),
            schema(SourceRecord::TYPE, schema_of::<SourceRecord>(settings())),
        ]
    }

    fn call(
        &self,
        host: &dyn Host,
        capability: &CapabilityName,
        input: Value,
    ) -> Result<Value, PluginError> {
        match capability.as_str() {
            "for-path" => for_path::for_path(host, &input),
            "for-paths" => for_paths::for_paths(host, &input),
            "check" => check::check(host, &input),
            "evaluate" => evaluate::evaluate(host, &input),
            "from-standards" => standards::from_standards(host, &input),
            other => Err(PluginError {
                code: "controls::call::unknown_capability".to_owned(),
                message: format!(
                    "controls offers no capability {other}; it offers for-path, for-paths, check, evaluate and from-standards"
                ),
                fields: Map::new(),
                hint: None,
            }),
        }
    }
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
        "types": [ControlRecord::TYPE, SourceRecord::TYPE],
        "capabilities": [{
            "name": "for-path",
            "description": "The controls that apply to a path.",
            "inputSchema": schema_of::<for_path::Input>(SchemaSettings::draft2020_12()),
            "outputSchema": schema_of::<for_path::Output>(settings()),
            "annotations": {"readOnlyHint": true, "openWorldHint": false},
            "_meta": {},
        }, {
            "name": "for-paths",
            "description": "The controls that apply to any of several files and SQLite GLOB patterns, \
                each once with its summary and the paths that reached it.",
            "inputSchema": schema_of::<for_paths::Input>(SchemaSettings::draft2020_12()),
            "outputSchema": schema_of::<for_paths::Output>(settings()),
            "annotations": {"readOnlyHint": true, "openWorldHint": false},
            "_meta": {},
        }, {
            "name": "check",
            "description": "Check the controls under a path, `/` when left out. Fails naming each \
                control over budget or degraded.",
            "inputSchema": schema_of::<check::Input>(SchemaSettings::draft2020_12()),
            "outputSchema": schema_of::<check::Output>(settings()),
            "annotations": {"readOnlyHint": true, "openWorldHint": false},
            "_meta": {},
        }, {
            "name": "evaluate",
            "description": "Check named parts of text against controls' budget, pattern and judged \
                rules, and return each control's verdict on each part with its findings.",
            "inputSchema": schema_of::<evaluate::Input>(SchemaSettings::draft2020_12()),
            "outputSchema": schema_of::<evaluate::Output>(settings()),
            "annotations": {"readOnlyHint": true, "openWorldHint": true},
            "_meta": {},
            "timeout": judged::EVALUATE_TIMEOUT,
            "env": [judged::AGENT_VARIABLE, "ANTHROPIC_API_KEY", "CLAUDE_CODE_OAUTH_TOKEN"],
        }, {
            "name": "from-standards",
            "description": "The controls a standards repository's rule tables give a repository that \
                adopts its profiles, as `board put --batch` lines that make a directory hold exactly them.",
            "inputSchema": schema_of::<standards::Input>(SchemaSettings::draft2020_12()),
            "outputSchema": schema_of::<standards::Output>(settings()),
            "annotations": {"readOnlyHint": true, "openWorldHint": false},
            "_meta": {},
        }],
    })
}

#[cfg(test)]
mod tests;
