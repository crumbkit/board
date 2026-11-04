//! Notes repos, their `notes-repo.json` records, and the git remotes the
//! records cover. Each function takes the calling plugin's name for its
//! codes, so any plugin that routes by remote can use it.
//!
//! A notes repo's `notes/repo/v1` record, which the notes plugin ships,
//! names the GitHub repositories whose work it is about.

use board_model::github_remote;
use board_ops::PluginError;
use serde_json::{Map, Value, json};

use crate::refusal;

/// Where a repo's record sits in its tree.
pub(crate) const RECORD: &str = "notes-repo.json";
pub(crate) const RECORD_TYPE: &str = "notes/repo/v1";

/// A repo: its root, the commit its mount pins and the clone Board read it
/// from (both none for the board's own tree), and its record's type,
/// status, Board code and body, each null without one.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Listed {
    pub(crate) root: String,
    pub(crate) source: Option<String>,
    pub(crate) clone: Option<String>,
    pub(crate) row: [Value; 4],
}

/// `value` as Python's `str` shows it: a string as itself, null as `None`.
pub(crate) fn shown(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// `path` without a trailing `/`, except the board's root itself.
pub(crate) fn root(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        "/".to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// The repos in `rows`, each its root, source, clone, and its record's
/// type, status, code and body, by root. Board's `sources` table names the
/// clone a commit was first read from.
pub(crate) fn parsed(rows: &[&[Value]]) -> Vec<Listed> {
    let mut found: Vec<Listed> = (rows.iter())
        .map(|row| {
            let mounted = row[1].as_str() != Some("worktree");
            Listed {
                root: root(&shown(&row[0])),
                source: mounted.then(|| shown(&row[1])),
                clone: mounted.then(|| shown(&row[2])),
                row: [
                    row[3].clone(),
                    row[4].clone(),
                    row[5].clone(),
                    row[6].clone(),
                ],
            }
        })
        .collect();
    found.sort_by(|a, b| (&a.root, &a.source, &a.clone).cmp(&(&b.root, &b.source, &b.clone)));
    found
}

/// The body of the record at `path`, which Board's `records` table holds
/// as JSON text, parsed as Board parses a record; one that does not parse
/// is `<plugin>::board::refused`.
pub(crate) fn json_body(plugin: &str, path: &str, body: &Value) -> Result<Value, PluginError> {
    let Value::String(text) = body else {
        return Ok(body.clone());
    };
    board_model::parse_json(text.as_bytes()).map_err(|error| {
        let why =
            format!("board query gave the body of {path} as text that does not parse: {error}");
        refusal(
            &format!("{plugin}::board::refused"),
            why,
            [("path", json!(path))],
        )
    })
}

/// The record of `listed`: none without one, and refused unless valid.
pub(crate) fn record(
    plugin: &str,
    listed: &Listed,
) -> Result<Option<Map<String, Value>>, PluginError> {
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
        &format!("{plugin}::repo::invalid_record"),
        why,
        fields,
    ))
}

/// Whether `record` covers the GitHub repository `remote` names, as
/// `owner/*` or `owner/name`, in any case.
pub(crate) fn covers(record: Option<&Map<String, Value>>, remote: Option<&str>) -> bool {
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

/// Whether `record` says `personal: true`.
fn personal(record: Option<&Map<String, Value>>) -> bool {
    record.and_then(|record| record.get("personal")) == Some(&Value::Bool(true))
}

/// The repos to read, the board's own tree, `/`, among them: the one
/// named; else every repo outside a git repository; else `/`, the personal
/// repo and the repos whose record covers the remote. Inside a git
/// repository whose origin is `unread`, no repo is covered, so they are `/`
/// and the personal repo.
pub(crate) fn scope(
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
                &format!("{plugin}::repo::not_found"),
                why,
                [("repos", json!(roots))],
            ));
        }
        return Ok(vec![root(name)]);
    }
    if remote.is_none() && unread.is_none() {
        return Ok(roots);
    }
    let mut records = Vec::new();
    for listed in found {
        records.push((listed.root.as_str(), record(plugin, listed)?));
    }
    let of = |at: &str| {
        records
            .iter()
            .find(|(root, _)| *root == at)
            .and_then(|(_, record)| record.as_ref())
    };
    Ok(roots
        .into_iter()
        .filter(|at| at == "/" || personal(of(at)) || covers(of(at), remote))
        .collect())
}
