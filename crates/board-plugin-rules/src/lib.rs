//! The rules plugin built into Board, formerly the controls plugin
//! (plugins/rules/README.md).

mod check;
mod evaluate;
mod for_path;
mod for_paths;
mod index;
mod judged;
mod migrate;
mod overlap;
pub mod records;
mod rename;
mod standards;
mod summary;
mod version;

use board_model::{CapabilityName, Code, Fields, LogicalPath, RecordType, Refusal};
use board_ops::{Host, Plugin, PluginError};
use board_plugin::Manifest;
use board_plugin::shape::{Record, schema_of};
use schemars::generate::SchemaSettings;
use serde_json::{Map, Value, json};

use records::{FORMER_SCHEMAS, RENAMED, RuleRecord, SourceRecord};

#[derive(Clone, Copy, Debug)]
pub struct Rules;

impl Plugin for Rules {
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
        let shipped = [
            schema(RuleRecord::TYPE, schema_of::<RuleRecord>(settings())),
            schema(SourceRecord::TYPE, schema_of::<SourceRecord>(settings())),
        ];
        let former = (RENAMED.iter().zip(FORMER_SCHEMAS)).map(|((old, _), text)| {
            schema(
                old,
                serde_json::from_str(text).expect("a committed schema; a test reads it"),
            )
        });
        shipped.into_iter().chain(former).collect()
    }

    fn moved(&self) -> &'static [(&'static str, &'static str)] {
        &[("/controls", "/rules")]
    }

    fn renamed_types(&self) -> &'static [(&'static str, &'static str)] {
        &RENAMED
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
            "migrate" => migrate::migrate(host, &input),
            other => Err(PluginError {
                code: "rules::call::unknown_capability".to_owned(),
                message: format!(
                    "rules offers no capability {other}; it offers for-path, for-paths, check, evaluate, from-standards and migrate"
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
        PluginError::board_refused("rules", "refs", &refusal)
    })
}

fn manifest() -> Value {
    let settings = || SchemaSettings::draft2020_12().for_serialize();
    json!({
        "$type": "board/plugin-manifest/v1",
        "name": "rules",
        "former_names": ["controls"],
        "version": "0.1.0",
        "description": "The rules plugin, formerly controls, keeps requirements on files as records in \
            the board that holds the files. Use it when you start work in a board or a repository it \
            mounts, and before you edit a file a rule may govern. Start with `board -C <board> rules \
            check --path /<directory>`, which lists the budgets it measures and the written requirements \
            under that path. `board -C <board> rules for-path --path /<file>` lists the rules on one \
            file. `board -C <board> rules evaluate` checks text, such as a draft, against the checkers \
            of the rules it names before the text is written. SKILL.md and README.md in the plugin's directory say more.",
        "metadata": {"area": "governance"},
        "types": [RuleRecord::TYPE, SourceRecord::TYPE, RENAMED[0].0, RENAMED[1].0],
        "capabilities": [{
            "name": "for-path",
            "description": "The rules that apply to a path.",
            "inputSchema": schema_of::<for_path::Input>(SchemaSettings::draft2020_12()),
            "outputSchema": schema_of::<for_path::Output>(settings()),
            "annotations": {
                "readOnlyHint": true,
                "destructiveHint": false,
                "idempotentHint": true,
                "openWorldHint": false,
            },
            "_meta": {},
        }, {
            "name": "for-paths",
            "description": "The rules that apply to any of several files and SQLite GLOB patterns, \
                each once with its summary and the paths that reached it.",
            "inputSchema": schema_of::<for_paths::Input>(SchemaSettings::draft2020_12()),
            "outputSchema": schema_of::<for_paths::Output>(settings()),
            "annotations": {
                "readOnlyHint": true,
                "destructiveHint": false,
                "idempotentHint": true,
                "openWorldHint": false,
            },
            "_meta": {},
        }, {
            "name": "check",
            "description": "Check the rules under a path, `/` when left out, measuring only \
                the files under it. A rule with no checker is described. `passed` is false \
                when a budget is over, a pin is behind or a file is unreadable; fails when a \
                rule is degraded or a version checker's capture is invalid.",
            "inputSchema": schema_of::<check::Input>(SchemaSettings::draft2020_12()),
            "outputSchema": schema_of::<check::Output>(settings()),
            "annotations": {
                "readOnlyHint": true,
                "destructiveHint": false,
                "idempotentHint": true,
                "openWorldHint": false,
            },
            "_meta": {},
        }, {
            "name": "evaluate",
            "description": "Check named parts of text against rules' budget, pattern, version and \
                judged checkers, and return each rule's verdict on each part with its findings.",
            "inputSchema": schema_of::<evaluate::Input>(SchemaSettings::draft2020_12()),
            "outputSchema": schema_of::<evaluate::Output>(settings()),
            "annotations": {
                "readOnlyHint": true,
                "destructiveHint": false,
                "idempotentHint": true,
                "openWorldHint": true,
            },
            "_meta": {},
            "timeout": judged::EVALUATE_TIMEOUT,
            "env": [judged::AGENT_VARIABLE, "ANTHROPIC_API_KEY", "CLAUDE_CODE_OAUTH_TOKEN"],
        }, {
            "name": "from-standards",
            "description": "The rules a standards repository's rule tables give a repository that \
                adopts its profiles, as `board put --batch` lines that make a directory hold exactly them.",
            "inputSchema": schema_of::<standards::Input>(SchemaSettings::draft2020_12()),
            "outputSchema": schema_of::<standards::Output>(settings()),
            "annotations": {
                "readOnlyHint": true,
                "destructiveHint": false,
                "idempotentHint": true,
                "openWorldHint": false,
            },
            "_meta": {},
        }, {
            "name": "migrate",
            "description": "owed_by sets each obligation's owed_by from what its text and sources \
                name; rename moves rules to /rules/ and rewrites every old name in the board's own \
                unsigned records. apply writes in one bulk commit, a rename only with its dry run's \
                digest; without apply nothing is written.",
            "inputSchema": schema_of::<migrate::Input>(SchemaSettings::draft2020_12()),
            "outputSchema": schema_of::<migrate::Output>(settings()),
            "annotations": {
                "readOnlyHint": false,
                "destructiveHint": true,
                "idempotentHint": true,
                "openWorldHint": false,
            },
            "_meta": {},
        }],
    })
}

#[cfg(test)]
mod tests;
