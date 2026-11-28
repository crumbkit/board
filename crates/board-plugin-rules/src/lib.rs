//! The rules plugin built into Board, formerly the controls plugin
//! (plugins/rules/README.md).

mod check;
mod evaluate;
mod for_path;
mod for_paths;
mod glob;
mod index;
mod judged;
mod migrate;
mod overlap;
pub mod records;
mod rename;
mod report;
mod standards;
mod summary;
mod version;
mod zero;

use board_model::{CapabilityName, Code, Fields, LogicalPath, RecordType, Refusal};
use board_ops::{Host, Plugin, PluginError};
use board_plugin::Manifest;
use board_plugin::shape::{Record, schema_of};
use schemars::generate::SchemaSettings;
use serde_json::{Map, Value, json};

use records::{FORMER_SCHEMAS, GroupRecord, RENAMED, RuleRecord, SourceRecord};
use report::{REPORT_V1, REPORT_V1_SCHEMA, ReportRecord};

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
            schema(GroupRecord::TYPE, schema_of::<GroupRecord>(settings())),
            // Every key of a report is present, and null means none.
            schema(
                ReportRecord::TYPE,
                schema_of::<ReportRecord>(settings().for_serialize()),
            ),
            // The report Board 0.1.3 printed, kept for readers of old reports.
            schema(
                REPORT_V1,
                serde_json::from_str(REPORT_V1_SCHEMA)
                    .expect("a committed schema; a test reads it"),
            ),
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
            "report" => report::report(host, &input),
            other => Err(PluginError {
                code: "rules::call::unknown_capability".to_owned(),
                message: format!(
                    "rules offers no capability {other}; it offers for-path, for-paths, check, evaluate, from-standards, migrate and report"
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

/// What the `check` capability does, as its manifest describes it.
const CHECK: &str = "Check the rules under a path, `/` when left out, measuring only the files \
    under it. A rule with no checker is described. `passed` is false when a budget is over, a pin \
    is behind, a file is unreadable or a zero rule's measures are not all 0 in its suite's latest \
    run; fails when a rule is degraded or a version checker's capture is invalid.";

/// The record types the plugin ships: its own, then those before the
/// rename.
const TYPES: [&str; 7] = [
    RuleRecord::TYPE,
    SourceRecord::TYPE,
    GroupRecord::TYPE,
    ReportRecord::TYPE,
    REPORT_V1,
    RENAMED[0].0,
    RENAMED[1].0,
];

/// What the `report` capability does, as its manifest describes it, with
/// the commands help shows at its top.
const REPORT: &str = "Print one rules/report/v2 document of every rule, group and compliance \
    suite that reaches a repository: each rule's result, each group's rolled-up state, each \
    suite's newest clean full run and the commits since it, where the repository's own rules \
    differ from the imposed set, every refusal, and the GitHub Actions run that made it, null \
    elsewhere. It writes nothing, and exits 0 whatever the rules found; it fails only when Board \
    refuses a read or the repository has no name. Examples:\n\
    board --checkout rules report --path /repos/app > rules-report.json\n\
    board rules report --path /repos/app --repository owner/app   # clone without origin\n\
    board rules report --path /repos/app --imposed /rules/imposed # set not at /rules\n\
    board rules report --path /                                   # repository is its own board\n\
    board schemas show rules/report/v2";

fn manifest() -> Value {
    let settings = || SchemaSettings::draft2020_12().for_serialize();
    json!({
        "$type": "board/plugin-manifest/v1",
        "name": "rules",
        "former_names": ["controls"],
        "version": "0.1.0",
        "description": "The plugin formerly called controls, which keeps requirements on files as \
            records in the board. Use it when you start work in a board or a mounted repository, \
            and before you edit a governed file.",
        "metadata": {"area": "governance"},
        "types": TYPES,
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
            "description": CHECK,
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
        }, report_capability()],
    })
}

/// The `report` capability, as the manifest lists it.
fn report_capability() -> Value {
    let settings = || SchemaSettings::draft2020_12().for_serialize();
    json!({
        "name": "report",
        "description": REPORT,
        "inputSchema": schema_of::<report::Input>(SchemaSettings::draft2020_12()),
        "outputSchema": schema_of::<ReportRecord>(settings()),
        "annotations": {
            "readOnlyHint": true,
            "destructiveHint": false,
            "idempotentHint": true,
            "openWorldHint": false,
        },
        "_meta": {},
        "env": report::CI_VARIABLES,
    })
}

#[cfg(test)]
mod tests;
