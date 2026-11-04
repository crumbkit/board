//! Notes repos, their `notes-repo.json` records, and the git remotes the
//! records cover.
//!
//! A notes repo is the board's root or a mount whose tree has a tracked
//! file under `daily/`, as in Reflect's layout. Its `notes/repo/v1` record,
//! which the notes plugin ships, names the GitHub repositories whose work
//! it is about. plugins/notes/README.md, under Routing a note, says how
//! notes uses them.

use board_model::github_remote;
use board_ops::{Host, Limit, PluginError, literal};
use serde_json::{Map, Value, json};

use crate::refusal;

/// Where a repo's record sits in its tree.
pub const RECORD: &str = "notes-repo.json";
pub const RECORD_TYPE: &str = "notes/repo/v1";

/// What [`query`] reads of each notes repo: its root, the commit its
/// mount pins and the clone Board read it from (`worktree` and the board's
/// own root for `/`), and its record's type, status, Board code and body.
pub const COLUMNS: [&str; 7] = [
    "n.root",
    "n.source",
    "s.repository",
    "e.type",
    "e.status",
    "e.code",
    "r.body",
];

/// The query that finds every notes repo, by root, in [`COLUMNS`], for a
/// caller that runs it within a query of its own.
pub fn query() -> String {
    let record = literal(RECORD);
    format!(
        "SELECT DISTINCT {} FROM (
  SELECT DISTINCT source, substr(logical, 1, length(logical) - length(path)) AS root FROM entries
  WHERE path GLOB 'daily/*' AND substr(logical, length(logical) - length(path) + 1) = path
) AS n
LEFT JOIN sources AS s ON s.source = n.source
LEFT JOIN entries AS e ON e.source = n.source AND e.path = {record}
LEFT JOIN records AS r ON r.source = n.source AND r.path = {record}
ORDER BY n.root",
        COLUMNS.join(", ")
    )
}

/// A repo: its root, the commit its mount pins and the clone Board read it
/// from (both none for the board's own tree), and its record's type,
/// status, Board code and body, each null without one.
#[derive(Clone, Debug, PartialEq)]
pub struct Listed {
    pub root: String,
    pub source: Option<String>,
    pub clone: Option<String>,
    pub row: [Value; 4],
}

/// `value` as Python's `str` shows it: a string as itself, null as `None`.
pub fn shown(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// `path` without a trailing `/`, except the board's root itself.
pub fn root(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        "/".to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// Every notes repo the board holds, by root.
pub fn listed(host: &dyn Host, plugin: &str) -> Result<Vec<Listed>, PluginError> {
    let report = (host.query(&query(), Limit::ALL))
        .map_err(|error| PluginError::board_refused(plugin, "query", &error.refusal()))?;
    let rows: Vec<&[Value]> = report.rows.iter().map(Vec::as_slice).collect();
    parsed(plugin, &rows)
}

/// The repos in `rows`, each in [`COLUMNS`], by root. Board's `sources`
/// table names the clone a commit was first read from. Board indexes a
/// commit once, so a second mount of a commit another mount already pins
/// has no entries and is not listed. A mount whose clone the table does
/// not name is `<plugin>::board::refused`.
pub fn parsed(plugin: &str, rows: &[&[Value]]) -> Result<Vec<Listed>, PluginError> {
    let mut found = Vec::new();
    for row in rows {
        let at = root(&shown(&row[0]));
        let mounted = row[1].as_str() != Some("worktree");
        let clone = match &row[2] {
            Value::Null if mounted => {
                let why = format!(
                    "Board names no clone for the mount at {at}; check the mount with `board \
                     check {at}`"
                );
                return Err(refusal(
                    format!("{plugin}::board::refused"),
                    why,
                    [("path", json!(at))],
                ));
            }
            clone => mounted.then(|| shown(clone)),
        };
        found.push(Listed {
            root: at,
            source: mounted.then(|| shown(&row[1])),
            clone,
            row: [
                row[3].clone(),
                row[4].clone(),
                row[5].clone(),
                row[6].clone(),
            ],
        });
    }
    found.sort_by(|a, b| (&a.root, &a.source, &a.clone).cmp(&(&b.root, &b.source, &b.clone)));
    Ok(found)
}

/// The body of the record at `path`, which Board's `records` table holds
/// as JSON text, parsed as Board parses a record; one that does not parse
/// is `<plugin>::board::refused`.
pub fn json_body(plugin: &str, path: &str, body: &Value) -> Result<Value, PluginError> {
    let Value::String(text) = body else {
        return Ok(body.clone());
    };
    board_model::parse_json(text.as_bytes()).map_err(|error| {
        let why =
            format!("board query gave the body of {path} as text that does not parse: {error}");
        refusal(
            format!("{plugin}::board::refused"),
            why,
            [("path", json!(path))],
        )
    })
}

/// The record of `listed`: none without one, and refused unless valid.
pub fn record(plugin: &str, listed: &Listed) -> Result<Option<Map<String, Value>>, PluginError> {
    let [kind, status, code, body] = &listed.row;
    if kind.is_null() && status.is_null() {
        return Ok(None);
    }
    let at = &listed.root;
    let path = format!(
        "{}/{}",
        at.trim_end_matches('/'),
        RECORD.trim_end_matches(".json")
    );
    if kind.as_str() == Some(RECORD_TYPE) && status.as_str() == Some("ok") {
        return Ok(json_body(plugin, &path, body)?.as_object().cloned());
    }
    let clone = if at == "/" {
        "the board".to_owned()
    } else {
        format!("the clone mounted at {at}")
    };
    let why = format!(
        "{path} is not a valid {RECORD_TYPE} record (type {}, status {}, code {}), so no call \
         can route by remote; run `board -C <{clone}> plugin enable notes` so it holds the schema, \
         fix the record, then `board mount update {at}`",
        shown(kind),
        shown(status),
        shown(code)
    );
    let fields = [
        ("path", json!(path)),
        ("code", code.clone()),
        ("status", status.clone()),
    ];
    Err(refusal(
        format!("{plugin}::repo::invalid_record"),
        why,
        fields,
    ))
}

/// Whether `record` covers the GitHub repository `remote` names, as
/// `owner/*` or `owner/name`, in any case.
pub fn covers(record: Option<&Map<String, Value>>, remote: Option<&str>) -> bool {
    let (Some(record), Some((owner, name))) = (record, remote.and_then(github_remote)) else {
        return false;
    };
    let (owner, name) = (owner.to_lowercase(), name.to_lowercase());
    let patterns = record.get("covers").and_then(Value::as_array);
    patterns.into_iter().flatten().any(|pattern| {
        let pattern = shown(pattern).to_lowercase();
        let (each, rest) = pattern.split_once('/').unwrap_or((pattern.as_str(), ""));
        each == owner && (rest == "*" || rest == name)
    })
}

/// A repo's `notes/repo/v1` record.
type Record = Map<String, Value>;

/// Whether `record` says `personal: true`.
fn personal(record: Option<&Map<String, Value>>) -> bool {
    record.and_then(|record| record.get("personal")) == Some(&Value::Bool(true))
}

/// Each listed repo's root and record, refused when one is not valid.
fn records(plugin: &str, found: &[Listed]) -> Result<Vec<(String, Option<Record>)>, PluginError> {
    (found.iter())
        .map(|listed| Ok((listed.root.clone(), record(plugin, listed)?)))
        .collect()
}

/// The roots of the listed repos whose record covers `remote`, and of
/// those whose record says `personal: true`.
pub fn covering(
    plugin: &str,
    found: &[Listed],
    remote: Option<&str>,
) -> Result<(Vec<String>, Vec<String>), PluginError> {
    let records = records(plugin, found)?;
    let roots = |keep: &dyn Fn(Option<&Record>) -> bool| -> Vec<String> {
        (records.iter())
            .filter(|(_, record)| keep(record.as_ref()))
            .map(|(at, _)| at.clone())
            .collect()
    };
    Ok((roots(&|record| covers(record, remote)), roots(&personal)))
}

/// The repos to read, the board's own tree, `/`, among them: the one
/// named; else every repo outside a git repository; else `/`, the personal
/// repo and the repos whose record covers the remote. Inside a git
/// repository whose origin is `unread`, no repo is covered, so they are `/`
/// and the personal repo.
pub fn scope(
    plugin: &str,
    found: &[Listed],
    name: Option<&str>,
    remote: Option<&str>,
    unread: Option<&str>,
) -> Result<Vec<String>, PluginError> {
    let mut roots = vec!["/".to_owned()];
    roots.extend(
        (found.iter())
            .filter(|listed| listed.root != "/")
            .map(|listed| listed.root.clone()),
    );
    if let Some(name) = name {
        if !roots.contains(&root(name)) {
            let why = format!(
                "no repo is at {name} (repos: {}); name the board's own tree, /, or one of the \
                 other repos listed",
                roots.join(", ")
            );
            return Err(refusal(
                format!("{plugin}::repo::not_found"),
                why,
                [("repos", json!(roots))],
            ));
        }
        return Ok(vec![root(name)]);
    }
    if remote.is_none() && unread.is_none() {
        return Ok(roots);
    }
    let records = records(plugin, found)?;
    let of = |at: &str| {
        (records.iter())
            .find(|(root, _)| root == at)
            .and_then(|(_, record)| record.as_ref())
    };
    Ok(roots
        .into_iter()
        .filter(|at| at == "/" || personal(of(at)) || covers(of(at), remote))
        .collect())
}

#[cfg(test)]
mod tests;
