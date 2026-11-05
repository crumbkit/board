//! A change: reading and writing its record, the records it references,
//! its newest plan and the day of the last sync.

use std::collections::BTreeMap;
use std::time::Duration;

use board_model::parse_json;
use board_ops::{Host, PluginError, literal};
use board_plugin::shape::Record;
use board_repos::Date;
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::board::{self, body, readable, rows, text, tree_codes};
use crate::records::{ChangeRecord, PlanRecord, SyncRecord};
use crate::refusal;

/// Where `sync` records the day it last ran.
pub(crate) const SYNC_PATH: &str = "/changes/sync";
pub(crate) const SYNC_COMMAND: &str = "board call changes.sync";
/// The board's own changes; a mounted board's changes sit under the mount
/// and are that board's to keep.
pub(crate) const OWN: &str = "/changes/";
/// What `referenced` says of a reference that reaches nothing.
pub(crate) const NOT_FOUND: &str = "board::path::not_found";

/// A record a change references: its type, `file` for a file that is not a
/// record and `directory` for a directory, and its body, empty for those.
pub(crate) type Found = (String, Map<String, Value>);

/// The records a change references, in its order, and each reference left
/// out with the code that says why, in its order too.
#[derive(Debug, Default)]
pub(crate) struct Referenced {
    pub(crate) records: Vec<(String, Found)>,
    pub(crate) unreadable: Vec<(String, String)>,
}

impl Referenced {
    pub(crate) fn get(&self, path: &str) -> Option<&Found> {
        (self.records.iter())
            .find(|(each, _)| each == path)
            .map(|(_, found)| found)
    }

    /// The referenced records of `record_type`, by path, in the order the
    /// change references them.
    pub(crate) fn of_type<'a>(
        &'a self,
        record_type: &'a str,
    ) -> impl Iterator<Item = (&'a str, &'a Map<String, Value>)> {
        (self.records.iter())
            .filter(move |(_, (found, _))| found == record_type)
            .map(|(path, (_, body))| (path.as_str(), body))
    }
}

/// The change at the logical `path` and the blob that holds it.
///
/// A path that holds no change is `changes::change::not_found`. A change
/// Board marks degraded for anything but a broken reference is
/// `changes::change::degraded`, since its fields cannot be trusted.
pub(crate) fn read(host: &dyn Host, path: &str) -> Result<(ChangeRecord, String), PluginError> {
    let sql = format!(
        "SELECT blob, code, body FROM records WHERE logical = {} AND type = {}",
        literal(path),
        literal(ChangeRecord::TYPE)
    );
    let found = rows(host, &sql)?;
    let Some(row) = found.first() else {
        return Err(refusal(
            "changes::change::not_found",
            format!(
                "no {} record is at {path}; name a change by its logical path, such as \
                 /changes/<name>, and list the open ones with `board call changes.open`",
                ChangeRecord::TYPE
            ),
            [("path", json!(path))],
        ));
    };
    let degraded = |code: &str| {
        refusal(
            "changes::change::degraded",
            format!(
                "the change at {path} is degraded with {code}; run `board check {path}` and fix it"
            ),
            [("path", json!(path)), ("code", json!(code))],
        )
    };
    if !readable(row.get(1)) {
        return Err(degraded(text(row.get(1)).unwrap_or_default()));
    }
    let change = parsed(row.get(2)).ok_or_else(|| degraded("record::invalid"))?;
    let blob = text(row.first()).unwrap_or_default().to_owned();
    Ok((change, blob))
}

/// The change a `body` column holds, or none when it does not read as one.
pub(crate) fn parsed(value: Option<&Value>) -> Option<ChangeRecord> {
    let value = parse_json(text(value)?.as_bytes()).ok()?;
    serde_json::from_value(value).ok()
}

/// Today's UTC day.
pub(crate) fn today(host: &dyn Host) -> Date {
    board_repos::today(host.settings().now, 0, Duration::ZERO)
}

/// `change` with `touched` set to today, which keeps it active under the
/// WIP limit.
pub(crate) fn touch(host: &dyn Host, mut change: ChangeRecord) -> ChangeRecord {
    change.touched = Some(today(host).to_string());
    change
}

/// The change's name, the last part of its logical path, such as `fix-it`
/// in `/changes/fix-it`.
pub(crate) fn name_in(path: &str) -> &str {
    let trimmed = path.trim_end_matches('/');
    trimmed.rsplit('/').next().unwrap_or(trimmed)
}

/// The only branch a prototype's builder may edit: `proto/<name>`.
pub(crate) fn branch_of(path: &str) -> String {
    format!("proto/{}", name_in(path))
}

/// The `put` line that writes `change` at `path` over the blob `expect`,
/// or as a new file with `none`.
pub(crate) fn line(path: &str, expect: &str, change: &ChangeRecord) -> Value {
    json!({"path": path, "expect": expect, "record": change})
}

/// Writes `change` at `path` over the blob `expect`.
pub(crate) fn write(
    host: &dyn Host,
    path: &str,
    expect: &str,
    change: &ChangeRecord,
) -> Result<(), PluginError> {
    board::put(host, &[line(path, expect, change)])
}

/// A plan's text path, its `uses` and its `paths`.
pub(crate) type Newest = (Option<String>, Vec<String>, Vec<String>);

/// The change's newest plan: its text's path, its `uses` and its `paths`,
/// or no text, uses or paths when it has no plan record.
pub(crate) fn newest(host: &dyn Host, path: &str) -> Result<Newest, PluginError> {
    let prefix = format!("{path}/plans/");
    let sql = format!(
        "SELECT body FROM records WHERE type = {} AND substr(logical, 1, {}) = {} \
         AND (code IS NULL OR code IN ({})) ORDER BY logical DESC LIMIT 1",
        literal(PlanRecord::TYPE),
        prefix.chars().count(),
        literal(&prefix),
        tree_codes()
    );
    let found = rows(host, &sql)?;
    let Some(row) = found.first() else {
        return Ok((None, Vec::new(), Vec::new()));
    };
    let plan = body(row.first());
    let strings = |key: &str| -> Vec<String> {
        (plan
            .get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten())
        .filter_map(|each| each.as_str().map(str::to_owned))
        .collect()
    };
    let text = plan.get("text").and_then(Value::as_str).map(str::to_owned);
    Ok((text, strings("uses"), strings("paths")))
}

/// Each entry `refs` reaches, as its type and body, in their order, and
/// each unreadable one with the code that says why.
pub(crate) fn referenced(host: &dyn Host, refs: &[String]) -> Result<Referenced, PluginError> {
    if refs.is_empty() {
        return Ok(Referenced::default());
    }
    let listed: Vec<String> = refs.iter().map(|each| literal(each)).collect();
    let sql = format!(
        "SELECT e.logical, e.type, e.code, r.body FROM entries AS e \
         LEFT JOIN records AS r ON r.logical = e.logical AND r.blob = e.blob \
         WHERE e.logical IN ({})",
        listed.join(", ")
    );
    let mut records: BTreeMap<String, Found> = BTreeMap::new();
    let mut codes: BTreeMap<String, String> = BTreeMap::new();
    for row in rows(host, &sql)? {
        let Some(logical) = text(row.first()) else {
            continue;
        };
        if readable(row.get(2)) {
            let found = text(row.get(1)).unwrap_or("file").to_owned();
            records.insert(logical.to_owned(), (found, body(row.get(3))));
            codes.remove(logical);
        } else if !records.contains_key(logical) {
            let code = text(row.get(2)).unwrap_or_default();
            codes.insert(logical.to_owned(), code.to_owned());
        }
    }
    let unfound: Vec<String> = (refs.iter())
        .filter(|each| !records.contains_key(*each) && !codes.contains_key(*each))
        .cloned()
        .collect();
    for (each, answer) in unfound.iter().zip(board::refs(host, &unfound)?) {
        if answer.is_some() {
            records.insert(each.clone(), ("directory".to_owned(), Map::new()));
        }
    }
    let mut found = Referenced::default();
    for each in refs {
        if found.records.iter().any(|(seen, _)| seen == each)
            || found.unreadable.iter().any(|(seen, _)| seen == each)
        {
            continue;
        }
        if let Some(record) = records.get(each) {
            found.records.push((each.clone(), record.clone()));
        } else {
            let code = codes.get(each).map_or(NOT_FOUND, String::as_str);
            found.unreadable.push((each.clone(), code.to_owned()));
        }
    }
    Ok(found)
}

/// The UTC day the last writing sync ran, and, when that is before today or
/// never, the warning and its command.
#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct LastSync {
    synced: Option<String>,
    stale: bool,
    code: Option<&'static str>,
    command: &'static str,
}

/// The day the last writing sync ran and whether that is before today, with
/// the warning when it is.
pub(crate) fn last_sync(host: &dyn Host) -> Result<LastSync, PluginError> {
    let sql = format!(
        "SELECT json_extract(body, '$.synced') FROM records WHERE logical = {} AND type = {}",
        literal(SYNC_PATH),
        literal(SyncRecord::TYPE)
    );
    let found = rows(host, &sql)?;
    let synced = found
        .first()
        .and_then(|row| text(row.first()))
        .map(str::to_owned);
    let today = today(host).to_string();
    let stale = synced.as_ref().is_none_or(|day| *day < today);
    Ok(LastSync {
        synced,
        stale,
        code: stale.then_some("changes::sync::stale"),
        command: SYNC_COMMAND,
    })
}
