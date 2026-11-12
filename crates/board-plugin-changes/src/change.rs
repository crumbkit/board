//! A change: reading and writing its record, its prototype record, the
//! records attached to its plans, its current plan and the day of the last
//! sync.

use std::collections::BTreeMap;
use std::time::Duration;

use board_model::parse_json;
use board_ops::{Host, PluginError, literal};
use board_plugin::shape::Record;
use board_repos::Date;
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::board::{self, body, readable, rows, text, tree_codes};
use crate::records::{
    AttemptRecord, ChangeRecord, ChangeV1, DONE, DROPPED, Kind, PULL_TYPES, PlanRecord,
    PrototypeRecord, RUN_V1, State, is_final,
};
use crate::refusal;

/// Where in Board's cache `sync` keeps the day it last ran, by board.
const SYNC_PLACE: &str = "board-changes/synced";
pub(crate) const SYNC_COMMAND: &str = "board changes sync";
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
    /// The type Board gives each unreadable record that has one.
    pub(crate) unreadable_types: BTreeMap<String, String>,
}

/// Whether `a` and `b` are the two types an attempt has had.
fn both_attempts(a: &str, b: &str) -> bool {
    [a, b]
        .iter()
        .all(|each| [AttemptRecord::TYPE, RUN_V1].contains(each))
}

impl Referenced {
    pub(crate) fn get(&self, path: &str) -> Option<&Found> {
        (self.records.iter())
            .find(|(each, _)| each == path)
            .map(|(_, found)| found)
    }

    /// The referenced records of `record_type`, by path, in the order the
    /// change references them. An attempt counts as one whichever of its two
    /// types it has, so a stored flow that names `changes/run/v1` still
    /// reads the attempts written as `changes/attempt/v1`.
    pub(crate) fn of_type<'a>(
        &'a self,
        record_type: &'a str,
    ) -> impl Iterator<Item = (&'a str, &'a Map<String, Value>)> {
        (self.records.iter())
            .filter(move |(_, (found, _))| {
                found == record_type || both_attempts(found, record_type)
            })
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
        "SELECT blob, code, body FROM records WHERE logical = {} AND type IN ({})",
        literal(path),
        change_types()
    );
    let found = rows(host, &sql)?;
    let Some(row) = found.first() else {
        return Err(refusal(
            "changes::change::not_found",
            format!(
                "no {} record is at {path}; name a change by its logical path, such as \
                 /changes/<name>, and list the open ones with `board changes open`",
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
    from_value(parse_json(text(value)?.as_bytes()).ok()?)
}

/// The change `value` holds, as a v2 or a v1, or none when it reads as
/// neither.
pub(crate) fn from_value(value: Value) -> Option<ChangeRecord> {
    if value.get("$type").and_then(Value::as_str) == Some(ChangeV1::TYPE) {
        return serde_json::from_value::<ChangeV1>(value)
            .ok()
            .map(Into::into);
    }
    serde_json::from_value(value).ok()
}

/// The SQL list of both record types of a change, for `type IN (...)`.
pub(crate) fn change_types() -> String {
    let types: Vec<String> = (crate::CHANGE_TYPES.iter())
        .map(|each| literal(each))
        .collect();
    types.join(", ")
}

/// What sits at a change's `<path>/prototype`, the one fact that makes the
/// change a prototype: nothing, a prototype record Board reads, with its
/// blob, or something else, with the code Board gives it.
pub(crate) enum Held {
    Nothing,
    Prototype(PrototypeRecord, String),
    Unreadable(String),
}

/// What a row of `entries` joined to its body holds, as [`Held`].
fn held_in(row: &[Value]) -> Held {
    let (blob, kind, code, body) = (row.first(), row.get(1), row.get(2), row.get(3));
    if !readable(code) {
        return Held::Unreadable(text(code).unwrap_or_default().to_owned());
    }
    if text(kind) != Some(PrototypeRecord::TYPE) {
        return Held::Unreadable("board::ref::wrong_type".to_owned());
    }
    match from_body::<PrototypeRecord>(body) {
        Some(record) => Held::Prototype(record, text(blob).unwrap_or_default().to_owned()),
        None => Held::Unreadable("board::record::invalid".to_owned()),
    }
}

/// The prototype record of the change at `path` and its blob, or none when
/// nothing is at `<path>/prototype`. Anything there that Board does not
/// read as a prototype record is `changes::change::degraded`, so a
/// prototype never passes for a build change.
pub(crate) fn prototype_of(
    host: &dyn Host,
    path: &str,
) -> Result<Option<(PrototypeRecord, String)>, PluginError> {
    let at = format!("{path}/prototype");
    let sql = format!(
        "SELECT e.blob, e.type, e.code, b.body FROM entries AS e \
         LEFT JOIN bodies AS b ON b.blob = e.blob WHERE e.logical = {}",
        literal(&at)
    );
    let found = rows(host, &sql)?;
    match found.first().map_or(Held::Nothing, |row| held_in(row)) {
        Held::Nothing => Ok(None),
        Held::Prototype(record, blob) => Ok(Some((record, blob))),
        Held::Unreadable(code) => Err(refusal(
            "changes::change::degraded",
            format!(
                "the prototype record of {path} at {at} is degraded with {code}, so whether the \
                 change is a prototype is not known; run `board check {at}` and fix it"
            ),
            [("path", json!(at)), ("code", json!(code))],
        )),
    }
}

/// The kind of the change at `path`: a prototype when its prototype record
/// is there.
pub(crate) fn kind_of(host: &dyn Host, path: &str) -> Result<Kind, PluginError> {
    Ok(if prototype_of(host, path)?.is_some() {
        Kind::Prototype
    } else {
        Kind::Build
    })
}

/// What sits at `<change>/prototype` for every change of the board's own
/// that has something there, by the change's path: the record, or none
/// when Board does not read it as one.
pub(crate) fn prototypes(
    host: &dyn Host,
) -> Result<BTreeMap<String, Option<PrototypeRecord>>, PluginError> {
    let suffix = "/prototype";
    let sql = format!(
        "SELECT e.blob, e.type, e.code, b.body, e.logical FROM entries AS e \
         LEFT JOIN bodies AS b ON b.blob = e.blob \
         WHERE substr(e.logical, 1, {}) = {} AND e.logical LIKE {}",
        OWN.len(),
        literal(OWN),
        literal(&format!("%{suffix}"))
    );
    let mut found = BTreeMap::new();
    for row in rows(host, &sql)? {
        let Some(change) = text(row.get(4)).and_then(|logical| logical.strip_suffix(suffix)) else {
            continue;
        };
        // Only `/changes/<name>/prototype` is a change's prototype.
        if change[OWN.len()..].contains('/') {
            continue;
        }
        let record = match held_in(&row) {
            Held::Prototype(record, _) => Some(record),
            Held::Nothing | Held::Unreadable(_) => None,
        };
        found.insert(change.to_owned(), record);
    }
    Ok(found)
}

/// The kind of the change at `path`: a prototype when anything sits at its
/// `<path>/prototype`, read or not, so an unread one is never a build
/// change.
pub(crate) fn kind_in(prototypes: &BTreeMap<String, Option<PrototypeRecord>>, path: &str) -> Kind {
    if prototypes.contains_key(path) {
        Kind::Prototype
    } else {
        Kind::Build
    }
}

/// Where a prototype stands, which its change's phase says: settled when
/// done, abandoned when dropped, and exploring otherwise.
pub(crate) fn state_of(phase: &str) -> State {
    match phase {
        DONE => State::Settled,
        DROPPED => State::Abandoned,
        _ => State::Exploring,
    }
}

/// A record Board reads as valid, or degraded only for a broken reference.
fn sound() -> String {
    format!("code IS NULL OR code IN ({})", tree_codes())
}

/// The record a `body` column holds, or none when it does not read as one.
fn from_body<R: serde::de::DeserializeOwned>(value: Option<&Value>) -> Option<R> {
    let value = parse_json(text(value)?.as_bytes()).ok()?;
    serde_json::from_value(value).ok()
}

/// The records attached to the change at `path`: those each of its plans
/// lists in `records`, oldest plan first, each once.
pub fn attached(host: &dyn Host, path: &str) -> Result<Vec<String>, PluginError> {
    let prefix = format!("{path}/plans/");
    let sql = format!(
        "SELECT body FROM records WHERE type = {} AND substr(logical, 1, {}) = {} AND ({}) \
         ORDER BY logical",
        literal(PlanRecord::TYPE),
        prefix.chars().count(),
        literal(&prefix),
        sound()
    );
    let mut found: Vec<String> = Vec::new();
    for row in rows(host, &sql)? {
        let plan: Option<PlanRecord> = from_body(row.first());
        for each in plan.and_then(|plan| plan.records).unwrap_or_default() {
            if !found.contains(&each) {
                found.push(each);
            }
        }
    }
    Ok(found)
}

/// The plan record at `path` and its blob, or none when no valid plan is
/// there.
pub(crate) fn plan_at(
    host: &dyn Host,
    path: &str,
) -> Result<Option<(PlanRecord, String)>, PluginError> {
    let sql = format!(
        "SELECT blob, body FROM records WHERE logical = {} AND type = {} AND ({})",
        literal(path),
        literal(PlanRecord::TYPE),
        sound()
    );
    let found = rows(host, &sql)?;
    Ok(found.first().and_then(|row| {
        let record = from_body(row.get(1))?;
        Some((record, text(row.first()).unwrap_or_default().to_owned()))
    }))
}

/// A pull request as its github/pull/v1 record names it: owner, repository,
/// number and page.
pub(crate) struct Pull {
    pub(crate) owner: String,
    pub(crate) repo: String,
    pub(crate) number: u64,
    pub(crate) url: String,
}
/// The github/pull/v1 record at `reference`, or none when Board reads no
/// such record there.
pub(crate) fn pull_of(host: &dyn Host, reference: &str) -> Result<Option<Pull>, PluginError> {
    if reference.is_empty() {
        return Ok(None);
    }
    let found = referenced(host, &[reference.to_owned()])?;
    let Some((_, body)) = (found.of_type(PULL_TYPES[0])).next() else {
        return Ok(None);
    };
    let field = |key: &str| body.get(key).and_then(Value::as_str).map(str::to_owned);
    Ok((|| {
        Some(Pull {
            owner: field("owner")?,
            repo: field("repo")?,
            number: body.get("number").and_then(Value::as_u64)?,
            url: field("url")?,
        })
    })())
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
/// or as a new file with `none`. A change Board read as a v1 is written as
/// one, so only `migrate` changes its type.
pub(crate) fn line(path: &str, expect: &str, change: &ChangeRecord) -> Value {
    let record = match change.as_v1().filter(|_| change.v1) {
        Some(old) => json!(old),
        None => json!(change),
    };
    json!({"path": path, "expect": expect, "record": record})
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
pub(crate) type Current = (Option<String>, Vec<String>, Vec<String>);

/// The change's current plan, the record its `plan` names: its text's
/// path, its `uses` and its `paths`, or no text, uses or paths when it names
/// none that Board reads. Every reader of the plan reads it here, so the
/// gates and the brief never read one plan's text and another's paths.
pub(crate) fn current(host: &dyn Host, change: &ChangeRecord) -> Result<Current, PluginError> {
    let Some(plan) = &change.plan else {
        return Ok((None, Vec::new(), Vec::new()));
    };
    Ok(
        plan_at(host, plan)?.map_or((None, Vec::new(), Vec::new()), |(record, _)| {
            (
                Some(record.text),
                record.uses,
                record.paths.unwrap_or_default(),
            )
        }),
    )
}

/// The records the change's current plan lists in `considered` and the
/// obligations it lists in `owed`, in their `/rules/` form, none when it
/// names no plan Board reads.
pub(crate) fn listed(
    host: &dyn Host,
    change: &ChangeRecord,
) -> Result<(Vec<String>, Vec<String>), PluginError> {
    let Some(plan) = &change.plan else {
        return Ok((Vec::new(), Vec::new()));
    };
    Ok(
        plan_at(host, plan)?.map_or((Vec::new(), Vec::new()), |(record, _)| {
            (
                (record.considered.into_iter().flatten())
                    .flat_map(|each| each.records)
                    .collect(),
                (record.owed.into_iter().flatten())
                    .map(|each| crate::rules::current(&each.rule))
                    .collect(),
            )
        }),
    )
}

/// Whether Board reads a record of `record_type` at `path` with no code at
/// all, so a record with a dangling reference does not count.
pub(crate) fn whole_record_at(
    host: &dyn Host,
    path: &str,
    record_type: &str,
) -> Result<bool, PluginError> {
    let sql = format!(
        "SELECT 1 FROM entries WHERE logical = {} AND type = {} AND code IS NULL",
        literal(path),
        literal(record_type)
    );
    Ok(!rows(host, &sql)?.is_empty())
}

/// Each entry `refs`, one change's references, reaches, as its type and body,
/// in their order, and each unreadable one with the code that says why. A
/// reference the tree stopped, such as one beneath a mount that cannot be
/// read, is `changes::board::refused`, so that no gate or sync takes it for
/// absent.
pub(crate) fn referenced(host: &dyn Host, refs: &[String]) -> Result<Referenced, PluginError> {
    gathered(host, refs, board::refs_strictly)
}

/// [`referenced`] for a walk over many changes, which passes over a
/// reference the tree stopped, as unreadable with the code that stopped it,
/// and warns of it.
pub(crate) fn referenced_in_walk(
    host: &dyn Host,
    refs: &[String],
) -> Result<Referenced, PluginError> {
    gathered(host, refs, board::refs_in_walk)
}

/// [`referenced`], with `resolve` answering the references no entry holds.
fn gathered(
    host: &dyn Host,
    refs: &[String],
    resolve: fn(&dyn Host, &[String]) -> board::Walked,
) -> Result<Referenced, PluginError> {
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
    let mut types: BTreeMap<String, String> = BTreeMap::new();
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
            if let Some(kind) = text(row.get(1)) {
                types.insert(logical.to_owned(), kind.to_owned());
            }
        }
    }
    let unfound: Vec<String> = (refs.iter())
        .filter(|each| !records.contains_key(*each) && !codes.contains_key(*each))
        .cloned()
        .collect();
    for (each, answer) in unfound.iter().zip(resolve(host, &unfound)?) {
        match answer {
            Ok(Some(_)) => {
                records.insert(each.clone(), ("directory".to_owned(), Map::new()));
            }
            Ok(None) => {}
            Err(code) => {
                codes.insert(each.clone(), code.as_str().to_owned());
            }
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
            if let Some(kind) = types.get(each) {
                found.unreadable_types.insert(each.clone(), kind.clone());
            }
        }
    }
    Ok(found)
}

/// The UTC day the last writing sync ran, and, when that is before today or
/// never and sync has a change to read, the warning and its command.
#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct LastSync {
    synced: Option<String>,
    stale: bool,
    code: Option<&'static str>,
    command: &'static str,
}

/// The key Board's cache keeps this board's sync day under: the SHA-256 of
/// the board's repository path, or none when Board names no root.
fn sync_key(host: &dyn Host) -> Result<Option<Vec<u8>>, PluginError> {
    Ok(board::resolve(host, "/")?.map(|report| {
        let root = report.location.board.repository;
        Sha256::digest(root.as_os_str().as_encoded_bytes()).to_vec()
    }))
}

/// Records `day` as the day of the last writing sync in Board's cache, or
/// says why the cache did not take it. A lost day only makes `next` and
/// `open` on this checkout warn that sync is stale until a sync records one.
pub(crate) fn record_sync(host: &dyn Host, day: &str) -> Result<Option<String>, PluginError> {
    let Some(key) = sync_key(host)? else {
        return Ok(None);
    };
    Ok(host
        .cache(SYNC_PLACE, &key, day.as_bytes())
        .err()
        .map(|error| {
            format!(
                "Board's cache did not take {day} as the day of this sync ({error}), so `next` and \
             `open` on this checkout warn that sync is stale until a sync records it"
            )
        }))
}

/// Whether `sync` reads the pull request of the change at `path`: a change
/// in this board's own tree, past its plan and not ended, that records a
/// pull request.
pub(crate) fn sync_reads(path: &str, change: &ChangeRecord) -> bool {
    path.starts_with(OWN)
        && !change.pulls().is_empty()
        && change.phase != "plan"
        && !is_final(&change.phase)
}

/// The day the last writing sync ran and whether that is before today, with
/// the warning when it is. With `reads` false sync has nothing to read for
/// the answer, so it is never stale.
pub(crate) fn last_sync(host: &dyn Host, reads: bool) -> Result<LastSync, PluginError> {
    let synced = (sync_key(host)?)
        .and_then(|key| host.cached(SYNC_PLACE, &key))
        .and_then(|bytes| String::from_utf8(bytes).ok());
    let today = today(host).to_string();
    let stale = reads && synced.as_ref().is_none_or(|day| *day < today);
    Ok(LastSync {
        synced,
        stale,
        code: stale.then_some("changes::sync::stale"),
        command: SYNC_COMMAND,
    })
}

#[cfg(test)]
pub(crate) mod tests;
