//! `migrate --flows`: replaces each change's own copy of its flow with a
//! reference to the flow's stored version.
//!
//! Before versions, `start` and `reflow` wrote the flow a change follows to
//! `<change>/flow`. For each `changes/change/v2` whose `flow` names a
//! record outside `/flows/`, the migration writes that flow's version at
//! `/flows/<name>/<version>` when the board lacks it, points the change at
//! it and removes the change's own copy. Identical copies share one
//! version. A copy that is not a flow Board reads is left in place and
//! listed under `unreadable`. A change or run whose rewritten record fails
//! its schema is left as it is, a change with its copy, and listed under
//! `left` with the reason. The rest go on.
//!
//! An attempt, stored as `changes/run/v1` or `changes/attempt/v1`, that
//! names its flow by name, as runs did before
//! versions, is rewritten to name the version its change follows. A run
//! that names a copy the migration replaces, as a run of a change not yet
//! migrated does, is rewritten to name that copy's version. A copy stays
//! while a run that names it cannot be rewritten.

use std::collections::{BTreeMap, BTreeSet};

use board_model::parse_json;
use board_ops::{Host, PluginError, literal};
use board_plugin::shape::Record;
use board_validate::Schema;
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{Map, Value, json};

use super::{LISTED, Left, compiled, conforms};
use crate::board::{self, readable, rows, text};
use crate::change::OWN;
use crate::flow::{Flow, blob_at, version_path, version_write};
use crate::records::{AttemptRecord, ChangeRecord, RUN_V1};

/// The reason the commit of a migration gives in its `Board-Bulk:`
/// trailer, without and with `flows`.
const BULK: &str = "migrate changes";
const BULK_FLOWS: &str = "migrate flows";
const BULK_PLANS: &str = "migrate plans";
const BULK_ATTEMPTS: &str = "migrate attempts";

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
/// name a version. `unreadable` lists each copy left in place. `left`
/// lists each change or run whose rewritten record fails its schema, and
/// each run whose change follows no version, with the reason.
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

/// The entry that says the record at `path` was left as it is, and `why`.
fn left_at(path: String, why: String) -> Left {
    Left {
        path,
        why,
        page: None,
        recorded: Vec::new(),
    }
}

/// One stored record: its path, blob and body.
pub(super) struct Stored {
    pub(super) path: String,
    pub(super) blob: String,
    pub(super) body: Map<String, Value>,
}

/// Each file of `record_type` under `/changes/`, readable or not, with its
/// body.
pub(super) fn stored(host: &dyn Host, record_type: &str) -> Result<Vec<Stored>, PluginError> {
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
    attempts: bool,
) -> Result<(Output, Vec<Value>, Vec<String>), PluginError> {
    let mut written: BTreeSet<String> = (planned.iter())
        .filter_map(|line| line.get("path").and_then(Value::as_str))
        .map(str::to_owned)
        .collect();
    let mut lines = Vec::new();
    let mut versions = Vec::new();
    let mut changes = Vec::new();
    let mut unreadable = Vec::new();
    let mut left = Vec::new();
    // The copies to remove, with their blobs, once the runs that name them
    // are known to be rewritten.
    let mut removals: Vec<(String, String)> = Vec::new();
    let schemas = compiled();
    // The version each change follows, and the version of each copy the
    // migration replaces.
    let mut followed: BTreeMap<String, String> = BTreeMap::new();
    let mut replaced: BTreeMap<String, String> = BTreeMap::new();
    for change in stored(host, ChangeRecord::TYPE)? {
        let Some(copy) = (change.body.get("flow").and_then(Value::as_str)).map(str::to_owned)
        else {
            continue;
        };
        if copy.starts_with(FLOWS) {
            followed.insert(change.path.clone(), copy);
            continue;
        }
        let Some((flow, blob)) = flow_at(host, &copy)? else {
            unreadable.push(Unreadable {
                path: copy,
                change: change.path.clone(),
                code: UNREADABLE,
            });
            continue;
        };
        let version = version_path(&flow);
        let mut body = change.body;
        body.insert("flow".to_owned(), json!(version));
        let repointed = json!({"path": change.path, "expect": change.blob, "record": body});
        // Checked before anything of the change is written, so a dry run
        // that lists a change promises an apply that writes it.
        if let Err(why) = conforms(&schemas, std::slice::from_ref(&repointed)) {
            left.push(left_at(change.path, why));
            continue;
        }
        if !written.contains(&version) && blob_at(host, &version)? == "none" {
            lines.push(version_write(&version, &flow));
            versions.push(version.clone());
        }
        written.insert(version.clone());
        if copy == format!("{}/flow", change.path) {
            removals.push((copy.clone(), blob));
        }
        lines.push(repointed);
        replaced.insert(copy, version.clone());
        followed.insert(change.path.clone(), version);
        changes.push(change.path);
    }
    let flows = Some((&followed, &replaced));
    let Rewritten {
        runs,
        retyped: attempted,
        kept,
    } = rewrite_runs(host, flows, attempts, &schemas, &mut lines, &mut left)?;
    let mut copies = 0;
    for (copy, blob) in removals {
        if !kept.contains(&copy) {
            lines.push(json!({"path": copy, "expect": blob, "remove": true}));
            copies += 1;
        }
    }
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
    Ok((output, lines, attempted))
}

/// The attempts whose flow was rewritten, those retyped, and the flows that
/// attempts left unrewritten still name.
struct Rewritten {
    runs: Vec<String>,
    retyped: Vec<String>,
    kept: BTreeSet<String>,
}

/// The version each change follows, and the version of each copy the
/// migration replaces.
type Versions<'a> = (&'a BTreeMap<String, String>, &'a BTreeMap<String, String>);

/// Adds to `lines` one rewrite of each attempt, stored as either of its types,
/// that needs one.
///
/// With `flows` (`followed`, `replaced`), an attempt that names its flow by
/// name is rewritten to name the version its change follows, and one that
/// names a copy in `replaced`, to name that copy's version. Adds to `left`
/// the attempts whose change follows no version. With `attempts`, an attempt
/// of `changes/run/v1` is also given `changes/attempt/v1`. Either change is
/// one line, so an attempt is written once. Adds to `left` each attempt whose
/// rewritten record fails its schema in `schemas`. Returns the attempts
/// whose flow was rewritten, those retyped, and the flows that attempts left
/// unrewritten still name, which must stay.
fn rewrite_runs(
    host: &dyn Host,
    flows: Option<Versions>,
    attempts: bool,
    schemas: &BTreeMap<String, Schema>,
    lines: &mut Vec<Value>,
    left: &mut Vec<Left>,
) -> Result<Rewritten, PluginError> {
    let (mut runs, mut retyped) = (Vec::new(), Vec::new());
    let mut kept = BTreeSet::new();
    let found = [AttemptRecord::TYPE, RUN_V1]
        .into_iter()
        .map(|kind| stored(host, kind))
        .collect::<Result<Vec<_>, _>>()?;
    let mut every: Vec<Stored> = found.into_iter().flatten().collect();
    every.sort_by(|a, b| a.path.cmp(&b.path));
    for run in every {
        let retype = attempts && run.body.get("$type").and_then(Value::as_str) == Some(RUN_V1);
        let named = (run.body.get("flow").and_then(Value::as_str)).map(str::to_owned);
        let version = match (flows, &named) {
            (Some((_, replaced)), Some(named)) if named.starts_with('/') => replaced.get(named),
            (Some((followed, _)), Some(named)) => {
                let change = run.path.rsplit_once("/runs/").map(|(change, _)| change);
                let version = change.and_then(|change| followed.get(change));
                if version.is_none() {
                    let why = format!(
                        "the run names the flow {named}, and its change follows no stored \
                         version of a flow; fix the change if `left` lists it, or reflow it \
                         with `board changes reflow`, then run migrate again"
                    );
                    left.push(left_at(run.path.clone(), why));
                }
                version
            }
            _ => None,
        };
        if version.is_none() && !retype {
            continue;
        }
        let mut body = run.body;
        if let Some(version) = version {
            body.insert("flow".to_owned(), json!(version));
        }
        if retype {
            body.insert("$type".to_owned(), json!(AttemptRecord::TYPE));
        }
        let rewritten = json!({"path": run.path, "expect": run.blob, "record": body});
        // The two types share one shape, and the plugin ships the schema of
        // only the new one, so the record is checked as an attempt.
        let mut checked = rewritten.clone();
        checked["record"]["$type"] = json!(AttemptRecord::TYPE);
        if let Err(why) = conforms(schemas, &[checked]) {
            left.push(left_at(run.path, why));
            kept.extend(named);
            continue;
        }
        lines.push(rewritten);
        if version.is_some() {
            runs.push(run.path.clone());
        }
        if retype {
            retyped.push(run.path);
        }
    }
    Ok(Rewritten {
        runs,
        retyped,
        kept,
    })
}

/// With `flows`, adds to `lines` the lines that replace the flow copies,
/// with `plans` those that move the plans' sections, with `attempts` those
/// that retype the attempts, and with `apply` writes `lines` in one bulk
/// commit; returns what each says, given only with its flag. Attempts
/// that fail their schema without `flows` are added to `left`.
pub(super) fn write(
    host: &dyn Host,
    (apply, flows, plans, attempts): (bool, bool, bool, bool),
    lines: &mut Vec<Value>,
    left: &mut Vec<Left>,
) -> Result<Written, PluginError> {
    let mut retyped = None;
    let replaced = if flows {
        let (output, written, attempted) = replace(host, lines, attempts)?;
        lines.extend(written);
        retyped = attempts.then_some(attempted);
        Some(output.applied(apply))
    } else {
        if attempts {
            let schemas = compiled();
            let done = rewrite_runs(host, None, true, &schemas, lines, left)?;
            retyped = Some(done.retyped);
        }
        None
    };
    let moved = if plans {
        Some(super::sections::rewrite(host, lines)?.applied(apply))
    } else {
        None
    };
    if apply && !lines.is_empty() {
        let bulk = match (flows, plans, attempts) {
            (true, _, _) => BULK_FLOWS,
            (false, true, _) => BULK_PLANS,
            (false, false, true) => BULK_ATTEMPTS,
            (false, false, false) => BULK,
        };
        board::put_bulk(host, lines, bulk)?;
    }
    Ok((replaced, moved, retyped))
}

/// What `write` reports: the flows, plan fields and attempts, each given
/// only with its flag.
pub(super) type Written = (
    Option<Output>,
    Option<super::sections::Output>,
    Option<Vec<String>>,
);
