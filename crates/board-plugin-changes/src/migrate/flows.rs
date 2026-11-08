//! `migrate --flows`: replaces each change's own copy of its flow with a
//! reference to the flow's stored version.
//!
//! Before versions, `start` and `reflow` wrote the flow a change follows to
//! `<change>/flow`. For each `changes/change/v2` whose `flow` names a
//! record outside `/flows/`, the migration writes that flow's version at
//! `/flows/<name>/<version>` when the board lacks it, points the change at
//! it and removes the change's own copy. Identical copies share one
//! version. A copy that is not a flow Board reads is left in place and
//! listed under `unreadable`, and the rest go on.
//!
//! A `changes/run/v1` that names its flow by name, as runs did before
//! versions, is rewritten to name the version its change follows.

use std::collections::{BTreeMap, BTreeSet};

use board_model::parse_json;
use board_ops::{Host, PluginError, literal};
use board_plugin::shape::Record;
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{Map, Value, json};

use super::{LISTED, Left};
use crate::board::{self, readable, rows, text};
use crate::change::OWN;
use crate::flow::{Flow, blob_at, version_path};
use crate::records::{ChangeRecord, RunRecord};

/// The reason the commit of a migration gives in its `Board-Bulk:`
/// trailer, without and with `flows`.
const BULK: &str = "migrate changes";
const BULK_FLOWS: &str = "migrate flows";

/// Where a board keeps its flows and their versions.
const FLOWS: &str = "/flows/";
/// The code of a copy that is not a flow Board reads.
const UNREADABLE: &str = "changes::flow::unreadable";

/// A copy left in place because it is not a flow Board reads: its path,
/// the change that names it and the code.
#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct Unreadable {
    path: String,
    change: String,
    code: &'static str,
}

/// What replacing the flow copies wrote, or would write.
///
/// `versions` lists the flow versions written. `changes` lists the changes
/// pointed at a version, at most 100, with `total` and `truncated`.
/// `copies` counts the copies removed. `runs` lists the runs rewritten to
/// name their change's version. `unreadable` lists each copy left in
/// place, and `left` each run whose change follows no version.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(rename = "MigrateFlowsOutput")]
pub(super) struct Output {
    applied: bool,
    versions: Vec<String>,
    changes: Vec<String>,
    total: usize,
    truncated: bool,
    copies: usize,
    runs: Vec<String>,
    unreadable: Vec<Unreadable>,
    left: Vec<Left>,
}

impl Output {
    /// The output, saying whether it was written.
    pub(super) fn applied(mut self, applied: bool) -> Self {
        self.applied = applied;
        self
    }
}

/// One stored record: its path, blob and body.
struct Stored {
    path: String,
    blob: String,
    body: Map<String, Value>,
}

/// Each file of `record_type` under `/changes/`, readable or not, with its
/// body.
fn stored(host: &dyn Host, record_type: &str) -> Result<Vec<Stored>, PluginError> {
    let sql = format!(
        "SELECT e.logical, e.blob, b.body FROM entries AS e JOIN bodies AS b ON b.blob = e.blob \
         WHERE e.type = {} AND substr(e.logical, 1, {}) = {} ORDER BY e.logical",
        literal(record_type),
        OWN.len(),
        literal(OWN)
    );
    let mut found = Vec::new();
    for row in rows(host, &sql)? {
        let parsed = text(row.get(2)).and_then(|body| parse_json(body.as_bytes()).ok());
        if let (Some(path), Some(blob), Some(Value::Object(body))) =
            (text(row.first()), text(row.get(1)), parsed)
        {
            found.push(Stored {
                path: path.to_owned(),
                blob: blob.to_owned(),
                body,
            });
        }
    }
    Ok(found)
}

/// The flow at `path` and its blob, or none when nothing Board reads as a
/// flow is there.
fn flow_at(host: &dyn Host, path: &str) -> Result<Option<(Flow, String)>, PluginError> {
    let sql = format!(
        "SELECT e.type, e.code, b.body, e.blob FROM entries AS e LEFT JOIN bodies AS b ON \
         b.blob = e.blob WHERE e.logical = {}",
        literal(path)
    );
    let found = rows(host, &sql)?;
    let Some(row) = found.first() else {
        return Ok(None);
    };
    if text(row.first()) != Some(Flow::TYPE) || !readable(row.get(1)) {
        return Ok(None);
    }
    let flow = (text(row.get(2)))
        .and_then(|body| parse_json(body.as_bytes()).ok())
        .and_then(|value| serde_json::from_value::<Flow>(value).ok());
    Ok(flow.zip(text(row.get(3)).map(str::to_owned)))
}

/// What replacing the copies would write, and its lines, leaving out a
/// version that one of `planned` already writes.
pub(super) fn replace(
    host: &dyn Host,
    planned: &[Value],
) -> Result<(Output, Vec<Value>), PluginError> {
    let mut written: BTreeSet<String> = (planned.iter())
        .filter_map(|line| line.get("path").and_then(Value::as_str))
        .map(str::to_owned)
        .collect();
    let mut lines = Vec::new();
    let mut versions = Vec::new();
    let mut changes = Vec::new();
    let mut unreadable = Vec::new();
    let mut copies = 0;
    let mut followed: BTreeMap<String, String> = BTreeMap::new();
    for change in stored(host, ChangeRecord::TYPE)? {
        let Some(copy) = change.body.get("flow").and_then(Value::as_str) else {
            continue;
        };
        if copy.starts_with(FLOWS) {
            followed.insert(change.path.clone(), copy.to_owned());
            continue;
        }
        let Some((flow, blob)) = flow_at(host, copy)? else {
            unreadable.push(Unreadable {
                path: copy.to_owned(),
                change: change.path.clone(),
                code: UNREADABLE,
            });
            continue;
        };
        let version = version_path(&flow);
        if !written.contains(&version) && blob_at(host, &version)? == "none" {
            lines.push(json!({"path": version, "expect": "none", "record": flow}));
            versions.push(version.clone());
        }
        written.insert(version.clone());
        if copy == format!("{}/flow", change.path) {
            lines.push(json!({"path": copy, "expect": blob, "remove": true}));
            copies += 1;
        }
        let mut body = change.body;
        body.insert("flow".to_owned(), json!(version));
        lines.push(json!({"path": change.path, "expect": change.blob, "record": body}));
        followed.insert(change.path.clone(), version);
        changes.push(change.path);
    }
    let (runs, left) = rewrite_runs(host, &followed, &mut lines)?;
    let total = changes.len();
    changes.truncate(LISTED);
    let output = Output {
        applied: false,
        versions,
        changes,
        total,
        truncated: total > LISTED,
        copies,
        runs,
        unreadable,
        left,
    };
    Ok((output, lines))
}

/// Adds to `lines` a rewrite of each run that names its flow by name, to
/// name the version its change follows, by `followed`; returns the runs
/// rewritten and those left because their change follows no version.
fn rewrite_runs(
    host: &dyn Host,
    followed: &BTreeMap<String, String>,
    lines: &mut Vec<Value>,
) -> Result<(Vec<String>, Vec<Left>), PluginError> {
    let (mut runs, mut left) = (Vec::new(), Vec::new());
    for run in stored(host, RunRecord::TYPE)? {
        let Some(named) = run.body.get("flow").and_then(Value::as_str) else {
            continue;
        };
        if named.starts_with('/') {
            continue;
        }
        let change = run.path.rsplit_once("/runs/").map(|(change, _)| change);
        let Some(version) = change.and_then(|change| followed.get(change)) else {
            left.push(Left {
                path: run.path.clone(),
                why: format!(
                    "the run names the flow {named}, and its change follows no stored version of \
                     a flow; reflow the change with `board call changes.reflow`, then run \
                     migrate again"
                ),
                page: None,
                recorded: Vec::new(),
            });
            continue;
        };
        let mut body = run.body;
        body.insert("flow".to_owned(), json!(version));
        lines.push(json!({"path": run.path, "expect": run.blob, "record": body}));
        runs.push(run.path);
    }
    Ok((runs, left))
}

/// With `flows`, adds to `lines` the lines that replace the flow copies,
/// and with `apply` writes `lines` in one bulk commit; returns what
/// replacing the copies says, given only with `flows`.
pub(super) fn write(
    host: &dyn Host,
    (apply, flows): (bool, bool),
    lines: &mut Vec<Value>,
) -> Result<Option<Output>, PluginError> {
    let replaced = if flows {
        let (output, written) = replace(host, lines)?;
        lines.extend(written);
        Some(output.applied(apply))
    } else {
        None
    };
    if apply && !lines.is_empty() {
        let bulk = if flows { BULK_FLOWS } else { BULK };
        board::put_bulk(host, lines, bulk)?;
    }
    Ok(replaced)
}
