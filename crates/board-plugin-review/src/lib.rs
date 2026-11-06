//! The review plugin built into Board (plugins/review/README.md): `run`
//! reviews a brief with a clean-context agent and records the report,
//! `post` comments a report on a pull request once the owner approves, and
//! `covered` checks that reports cover a pull request's commits. Every git
//! command goes through `git`.

mod covered;
mod git;
mod post;
pub mod records;
mod run;

use std::path::PathBuf;

use board_model::{CapabilityName, RecordType};
use board_ops::{Host, Limit, Plugin, PluginError, literal};
use board_plugin::Manifest;
use board_plugin::shape::{Record, TREE_CODES, schema_of};
use schemars::JsonSchema;
use schemars::generate::SchemaSettings;
use serde_json::{Map, Value, json};

use records::{BriefRecord, ReportRecord};

/// The plugin's name, which its codes start with.
const PLUGIN: &str = "review";
/// The record type of a change, which the changes plugin owns, and the
/// severities that shut its review gate, so an attached report with one
/// never covers.
use board_plugin_changes::{BLOCKING, CHANGE_TYPES};

#[derive(Clone, Copy, Debug)]
pub struct Review;

/// One capability: its name, what it does, the function that runs it, its
/// input and output schemas, its MCP hints (read-only, destructive,
/// idempotent, open-world), the variables it is given and how long it may
/// run.
struct Capability {
    name: &'static str,
    description: &'static str,
    run: fn(&dyn Host, &Value) -> Result<Value, PluginError>,
    input: fn() -> Value,
    output: fn() -> Value,
    hints: [bool; 4],
    env: &'static [&'static str],
    timeout: Option<u64>,
}

fn input<T: JsonSchema>() -> Value {
    schema_of::<T>(SchemaSettings::draft2020_12())
}

fn output<T: JsonSchema>() -> Value {
    schema_of::<T>(SchemaSettings::draft2020_12().for_serialize())
}

/// Every capability, in the order the manifest lists them.
const CAPABILITIES: [Capability; 3] = [
    Capability {
        name: "run",
        description: "Review the range or pinned record a brief names with the agent the \
            BOARD_REVIEW_AGENT variable names, and write the report beside the brief.",
        run: run::run,
        input: input::<run::Input>,
        output: output::<run::Output>,
        // It only adds a report. A retry sends the prompt to the model
        // provider again, and two runs at once both start the agent, so it
        // is not idempotent. The agent sends the brief and the diff to its
        // model provider.
        hints: [false, false, false, true],
        env: &run::ENV,
        timeout: Some(run::TIMEOUT),
    },
    Capability {
        name: "post",
        description: "Comment a report on a GitHub pull request with gh, once the owner approves \
            the whole comment through `board decide`, and set the report's source to the \
            comment's URL. A report whose source is set is skipped.",
        run: post::post,
        input: input::<post::Input>,
        output: output::<post::Output>,
        // It adds a comment and the report's source. A gh call that failed
        // may still have posted, and a retry posts the comment again, so it
        // is not idempotent.
        hints: [false, false, false, true],
        env: &post::ENV,
        timeout: Some(post::TIMEOUT),
    },
    Capability {
        name: "covered",
        description: "The reports in the board's own tree that cover every commit from the \
            merge base of a base and a head to the head, each by a report's range or patch ids, \
            after skipping commits that only add review records. With --change, the reports the \
            change attaches, and the commits of the clone its mount reads. A report asking for \
            changes never covers. Fails when a commit is uncovered.",
        run: covered::covered,
        input: input::<covered::Input>,
        output: output::<covered::Output>,
        hints: [true, false, true, false],
        env: &[],
        timeout: None,
    },
];

impl Plugin for Review {
    fn manifest(&self) -> Manifest {
        Manifest::built_in(manifest()).expect(
            "the manifest is built from constants and its inputs' schemas; a test parses it",
        )
    }

    fn schemas(&self) -> Vec<(RecordType, Value)> {
        let settings = SchemaSettings::draft2020_12;
        vec![
            (
                (BriefRecord::TYPE.parse()).expect("TYPE is a record type; a test parses it"),
                schema_of::<BriefRecord>(settings()),
            ),
            (
                (ReportRecord::TYPE.parse()).expect("TYPE is a record type; a test parses it"),
                schema_of::<ReportRecord>(settings()),
            ),
        ]
    }

    fn call(
        &self,
        host: &dyn Host,
        capability: &CapabilityName,
        input: Value,
    ) -> Result<Value, PluginError> {
        let found = (CAPABILITIES.iter()).find(|offered| offered.name == capability.as_str());
        let Some(offered) = found else {
            return Err(refusal(
                "review::call::unknown_capability",
                format!(
                    "review offers no capability {capability}; it offers run, post and covered"
                ),
                [],
            ));
        };
        (offered.run)(host, &input)
    }
}

fn manifest() -> Value {
    let capabilities: Vec<Value> = (CAPABILITIES.iter())
        .map(|offered| {
            let [read_only, destructive, idempotent, open_world] = offered.hints;
            let mut tool = json!({
                "name": offered.name,
                "description": offered.description,
                "inputSchema": (offered.input)(),
                "outputSchema": (offered.output)(),
                "annotations": {
                    "readOnlyHint": read_only,
                    "destructiveHint": destructive,
                    "idempotentHint": idempotent,
                    "openWorldHint": open_world,
                },
                "_meta": {},
            });
            if !offered.env.is_empty() {
                tool["env"] = json!(offered.env);
            }
            if let Some(timeout) = offered.timeout {
                tool["timeout"] = json!(timeout);
            }
            tool
        })
        .collect();
    json!({
        "$type": "board/plugin-manifest/v1",
        "name": PLUGIN,
        "version": "0.2.0",
        "description": "Review briefs, the reports a clean-context agent or a person writes for \
            them, and whether a commit is covered by reports. `board call review.run --brief \
            /reviews/<id>/brief` reviews the brief's range or pinned record with the agent \
            BOARD_REVIEW_AGENT names and records the report; `board call review.post --report \
            /reviews/<id>/report --pr <url>` comments it on a pull request once the owner \
            approves the comment; `board call review.covered --base <commit> --head <commit>` \
            checks that committed reports, or those a change attaches, cover a pull request's \
            commits. plugins/review/README.md in Board's repository says more.",
        "metadata": {"area": "review"},
        "types": [BriefRecord::TYPE, ReportRecord::TYPE],
        "capabilities": capabilities,
    })
}

/// A refusal of the plugin's own, with `fields`.
pub(crate) fn refusal<const N: usize>(
    code: &str,
    message: String,
    fields: [(&str, Value); N],
) -> PluginError {
    PluginError {
        code: code.to_owned(),
        message,
        fields: (fields.into_iter())
            .map(|(name, value)| (name.to_owned(), value))
            .collect::<Map<_, _>>(),
        hint: None,
    }
}

/// `review::board::refused`, for Board's refusal of `board <command>`.
pub(crate) fn refused(command: &str, refusal: &board_model::Refusal) -> PluginError {
    PluginError::board_refused(PLUGIN, command, refusal)
}

/// Every row `sql` selects.
pub(crate) fn rows(host: &dyn Host, sql: &str) -> Result<Vec<Vec<Value>>, PluginError> {
    let report = host.query(sql, Limit::ALL);
    Ok(report
        .map_err(|error| refused("query", &error.refusal()))?
        .rows)
}

/// The SQL that admits a record Board reads: whole, or degraded only by
/// what the tree around it holds.
pub(crate) fn readable() -> String {
    let codes: Vec<String> = TREE_CODES.iter().map(|code| literal(code)).collect();
    format!("(status = 'ok' OR code IN ({}))", codes.join(", "))
}

/// The valid record of `record_type` at `path` in the board's own tree, and
/// its blob, else `review::<area>::not_found`.
pub(crate) fn read_record(
    host: &dyn Host,
    path: &str,
    record_type: &str,
    area: &str,
) -> Result<(Value, String), PluginError> {
    let sql = format!(
        "SELECT body, blob FROM records WHERE source = 'worktree' AND logical = {} AND type = {} \
         AND {}",
        literal(path),
        literal(record_type),
        readable()
    );
    let found = rows(host, &sql)?.into_iter().next().and_then(|row| {
        let body = serde_json::from_str(row.first()?.as_str()?).ok()?;
        Some((body, row.get(1)?.as_str()?.to_owned()))
    });
    found.ok_or_else(|| {
        refusal(
            &format!("review::{area}::not_found"),
            format!(
                "no valid {record_type} is in the board's own tree at {path}; `board get {path}` \
                 and `board check` show what is there, and README.md says what the record holds"
            ),
            [("path", json!(path))],
        )
    })
}

/// The working directory of the repository that holds the board.
pub(crate) fn root(host: &dyn Host) -> Result<PathBuf, PluginError> {
    let found = rows(
        host,
        "SELECT repository FROM sources WHERE source = 'worktree'",
    )?;
    let root = (found.first().and_then(|row| row.first()?.as_str()))
        .expect("the index lists the board's own working tree among its sources");
    Ok(PathBuf::from(root))
}

#[cfg(test)]
mod tests;
