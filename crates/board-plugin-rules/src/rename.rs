//! `rules.migrate --rename true`: rewrites the board's own records from the
//! names before the rename to the names after it, history included, in one
//! commit with a `Board-Bulk:` trailer.
//!
//! Each rule and source moves from `/controls/` to `/rules/` under its new
//! type, and a rule's `rule` becomes its `checker`. A boundary's
//! `checks[].control`, a crossing verdict's `control` and `rule`, a plan's
//! `owed[].control` and a flow check's `controls` and former kinds take
//! their new names. Every string in a record, and every Markdown file, has
//! each `/controls/` path, `controls.<capability>` and old type name
//! rewritten. A flow version stored at the hash of its JSON is stored again
//! at the hash of its rewritten JSON, and every record that names the old
//! version names the new one, so each change is pinned to it.
//!
//! Signed records, attention orders and effects, and the files they name,
//! are never rewritten; those that hold an old name are listed. A mount
//! pointer under `/controls/`, and a board config that names `controls`,
//! which only the owner may write, are listed for a person to change. Only
//! the board's own working tree is read and written, never a mount.
//!
//! The dry run lists every write and a digest of them. `--apply true` takes
//! that digest and refuses with `rules::migrate::stale`, writing nothing,
//! when the writes it would make differ, as when a record changed since the
//! dry run; each line also expects the blob it read.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::sync::LazyLock;

use board_model::{
    CONFIG_TYPE, EFFECT_TYPES, LogicalPath, ORDER_TYPE, canonical_json, flow_version, parse_json,
};
use board_ops::{Bulk, Host, PluginError};
use regex::Regex;
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::records::{CONTROL_TYPE, RENAMED};

/// The reason the commit gives in its `Board-Bulk:` trailer.
const BULK: &str = "rename controls to rules";
const STALE: &str = "rules::migrate::stale";
const FLOW_TYPE: &str = "changes/flow/v1";
/// Each former check kind of a flow, and its name now.
const KINDS: [(&str, &str); 2] = [
    ("controlled-paths", "rule-paths"),
    ("planned-controlled", "planned-rule-paths"),
];

fn compiled(pattern: &str) -> Regex {
    Regex::new(pattern).expect("each pattern is a constant; the rename tests compile it")
}

/// Each rewrite of text, in order: crumbkit/board's inner `rules/` folder,
/// which its own tree dropped, then a `/controls/` path in this board or a
/// mounted one, a capability, the command word and the old types.
static TEXT: LazyLock<[(Regex, &str); 7]> = LazyLock::new(|| {
    [
        (
            compiled(r"/repos/board/controls/rules/"),
            "/repos/board/rules/",
        ),
        (
            compiled(r"(^|[^A-Za-z0-9_./-])controls/rules/"),
            "${1}rules/",
        ),
        (
            compiled(
                r"(^|[^A-Za-z0-9_.-])((?:/repos/[A-Za-z0-9_.-]+)?)/controls(/|$|[^A-Za-z0-9_.-])",
            ),
            "${1}${2}/rules${3}",
        ),
        (
            compiled(r"\bcontrols\.(check|for-paths|for-path|evaluate|from-standards|migrate)\b"),
            "rules.${1}",
        ),
        (compiled(r"\bboard controls "), "board rules "),
        (compiled(r"\bcontrols/control/v1\b"), "rules/rule/v1"),
        (compiled(r"\bcontrols/source/v1\b"), "rules/source/v1"),
    ]
});

/// One write: the path rewritten, and where it moves when it moves.
#[derive(Debug, PartialEq, Serialize, JsonSchema)]
pub(crate) struct Write {
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    to: Option<String>,
}

/// A path left as it is, and why.
#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct Kept {
    path: String,
    why: String,
}

/// What the rename writes or would write.
#[derive(Debug, Default)]
pub(crate) struct Renamed {
    pub(crate) writes: Vec<Write>,
    pub(crate) digest: String,
    pub(crate) signed: Vec<String>,
    pub(crate) kept: Vec<Kept>,
}

/// One entry of the board's own tree, with its text.
struct Entry {
    logical: String,
    blob: String,
    kind: String,
    role: Option<String>,
    record_type: Option<String>,
    text: String,
}

/// Lists the rename's writes and, with `apply`, writes them when `digest`
/// is the dry run's.
pub(crate) fn rename(
    host: &dyn Host,
    apply: bool,
    digest: Option<&str>,
) -> Result<Renamed, PluginError> {
    let entries = entries(host)?;
    let held: BTreeSet<&str> = entries.iter().map(|entry| entry.logical.as_str()).collect();
    let mut renamed = Renamed::default();
    let mut lines = Vec::new();
    let skipped = signed(&entries, &mut renamed.signed);
    let flows = flows(&entries);
    for entry in &entries {
        if skipped.contains(entry.logical.as_str())
            || entry.logical.starts_with("/.board/types/")
            || matches!(entry.kind.as_str(), "schema" | "json")
        {
            continue;
        }
        if entry.record_type.as_deref() == Some(CONFIG_TYPE) || entry.kind == "pointer" {
            renamed.kept.extend(left_for_a_person(entry, &flows));
            continue;
        }
        let to = flows
            .get(&entry.logical)
            .cloned()
            .or_else(|| moved(&entry.logical));
        let Some(content) = content(entry, to.is_some(), &flows) else {
            continue;
        };
        let line = |path: &str, expect: &str| {
            let mut line = content.clone();
            line["path"] = json!(path);
            line["expect"] = json!(expect);
            line
        };
        match &to {
            Some(to) if held.contains(to.as_str()) && flows.contains_key(&entry.logical) => {}
            Some(to) if held.contains(to.as_str()) => {
                renamed.kept.push(Kept {
                    path: entry.logical.clone(),
                    why: format!("{to} already holds a file; move or remove one by hand"),
                });
                continue;
            }
            Some(to) => lines.push(line(to, "none")),
            None => lines.push(line(&entry.logical, &entry.blob)),
        }
        if to.is_some() {
            lines.push(json!({"path": entry.logical, "expect": entry.blob, "remove": true}));
        }
        renamed.writes.push(Write {
            path: entry.logical.clone(),
            to,
        });
    }
    renamed.digest = digest_of(&lines);
    if apply {
        write(host, &lines, digest == Some(renamed.digest.as_str()))?;
    }
    Ok(renamed)
}

/// What a config or a pointer leaves for a person to change: a config that
/// names `controls`, which only the owner may write, or a pointer under
/// `/controls/`.
fn left_for_a_person(entry: &Entry, flows: &BTreeMap<String, String>) -> Option<Kept> {
    let why = if entry.kind == "pointer" {
        moved(&entry.logical)?;
        "a mount pointer; move it to /rules/ with board put once its target board has migrated"
    } else {
        let config = parse_json(entry.text.as_bytes()).unwrap_or_default();
        (rewritten(Some(CONFIG_TYPE), config.clone(), flows) != config).then_some(())?;
        "the board's config names controls; only the owner may write it, so name rules in its \
         place in .board/board.json"
    };
    Some(Kept {
        path: entry.logical.clone(),
        why: why.to_owned(),
    })
}

/// What a line writes for `entry`: its rewritten record, or its rewritten
/// text for Markdown or a file that moves; none when nothing changes.
fn content(entry: &Entry, moves: bool, flows: &BTreeMap<String, String>) -> Option<Value> {
    if entry.kind == "record" && entry.role.as_deref() == Some("json") {
        let Ok(Value::Object(body)) = parse_json(entry.text.as_bytes()) else {
            return None;
        };
        let body = Value::Object(body);
        let record = rewritten(entry.record_type.as_deref(), body.clone(), flows);
        (moves || record != body).then(|| json!({"record": record}))
    } else if entry.role.as_deref() == Some("markdown") || moves {
        let text = text(&entry.text, flows);
        (moves || text != entry.text).then(|| json!({"text": text}))
    } else {
        None
    }
}

/// Writes `lines` in one bulk commit, or refuses with
/// [`STALE`] when the digest given is not theirs.
fn write(host: &dyn Host, lines: &[Value], current: bool) -> Result<(), PluginError> {
    if !current {
        return Err(PluginError {
            code: STALE.to_owned(),
            message: "the writes differ from those the dry run listed, so a record changed since \
                      it ran; nothing was written"
                .to_owned(),
            fields: Map::new(),
            hint: Some(
                "run rules.migrate --rename true again and apply with its digest".to_owned(),
            ),
        });
    }
    if !lines.is_empty() {
        let refused = |refusal: &board_model::Refusal| {
            PluginError::board_refused("rules", "put --batch --bulk", refusal)
        };
        let bulk = Bulk::new(BULK).map_err(|error| refused(&error.refusal()))?;
        (host.put_bulk(lines, &bulk)).map_err(|error| refused(&error.refusal()))?;
    }
    Ok(())
}

/// The board's own records, Markdown files, pointers and files under
/// `/controls/`, with their text.
fn entries(host: &dyn Host) -> Result<Vec<Entry>, PluginError> {
    let sql = "SELECT e.logical, e.blob, e.kind, e.role, e.type, b.body FROM entries AS e \
               LEFT JOIN bodies AS b ON b.blob = e.blob WHERE e.source = 'worktree' \
               AND e.kind IN ('record', 'file', 'pointer') ORDER BY e.logical";
    let rows = crate::migrate::rows(host, sql)?;
    let text = |value: Option<&Value>| value.and_then(Value::as_str).map(str::to_owned);
    let mut entries: Vec<Entry> = (rows.iter())
        .filter_map(|row| {
            Some(Entry {
                logical: text(row.first())?,
                blob: text(row.get(1))?,
                kind: text(row.get(2))?,
                role: text(row.get(3)),
                record_type: text(row.get(4)),
                text: text(row.get(5)).unwrap_or_default(),
            })
        })
        .filter(|entry| {
            entry.kind != "file"
                || entry.role.as_deref() == Some("markdown")
                || moved(&entry.logical).is_some()
        })
        .collect();
    let unread: Vec<usize> = (entries.iter().enumerate())
        .filter(|(_, entry)| entry.kind != "pointer" && entry.role.as_deref() != Some("json"))
        .map(|(at, _)| at)
        .collect();
    let paths: Vec<LogicalPath> = (unread.iter())
        .filter_map(|at| entries[*at].logical.parse().ok())
        .collect();
    let read = (host.get_each(&paths))
        .map_err(|error| PluginError::board_refused("rules", "get", &error.refusal("get")))?;
    for (at, got) in unread.into_iter().zip(read) {
        if let Ok(got) = got {
            entries[at].text = String::from_utf8_lossy(&got.bytes).into_owned();
        }
    }
    Ok(entries)
}

/// The paths of the signed records and of every file they name, which the
/// rename never rewrites; each signed record that holds an old name goes in
/// `listed`.
fn signed<'entries>(
    entries: &'entries [Entry],
    listed: &mut Vec<String>,
) -> BTreeSet<&'entries str> {
    let mut skipped = BTreeSet::new();
    let named: BTreeSet<String> = (entries.iter())
        .filter(|entry| {
            let record_type = entry.record_type.as_deref().unwrap_or_default();
            record_type == ORDER_TYPE || EFFECT_TYPES.contains(&record_type)
        })
        .flat_map(|entry| {
            skipped.insert(entry.logical.as_str());
            if text(&entry.text, &BTreeMap::new()) != entry.text {
                listed.push(entry.logical.clone());
            }
            let strings = parse_json(entry.text.as_bytes())
                .map(strings)
                .unwrap_or_default();
            strings.into_iter().filter(|each| each.starts_with('/'))
        })
        .collect();
    skipped.extend(
        (entries.iter())
            .filter(|entry| named.contains(&entry.logical))
            .map(|entry| entry.logical.as_str()),
    );
    skipped
}

/// Every string in `value`.
fn strings(value: Value) -> Vec<String> {
    match value {
        Value::String(text) => vec![text],
        Value::Array(each) => each.into_iter().flat_map(strings).collect(),
        Value::Object(fields) => fields
            .into_iter()
            .flat_map(|(_, each)| strings(each))
            .collect(),
        _ => Vec::new(),
    }
}

/// Each flow version stored at the hash of its JSON whose rewritten JSON
/// has another hash, and that hash's path.
fn flows(entries: &[Entry]) -> BTreeMap<String, String> {
    let none = BTreeMap::new();
    (entries.iter())
        .filter(|entry| entry.record_type.as_deref() == Some(FLOW_TYPE))
        .filter_map(|entry| {
            let value = parse_json(entry.text.as_bytes()).ok()?;
            let name = value.get("name")?.as_str()?.to_owned();
            (flow_version(&name, &value) == entry.logical).then_some(())?;
            let stored = flow_version(&name, &rewritten(Some(FLOW_TYPE), value, &none));
            (stored != entry.logical).then(|| (entry.logical.clone(), stored))
        })
        .collect()
}

/// Where a path under `/controls/` moves: the same path under `/rules/`.
fn moved(path: &str) -> Option<String> {
    path.strip_prefix("/controls/")
        .map(|rest| format!("/rules/{rest}"))
}

/// `text` with every old path, capability and type renamed, and every flow
/// version in `flows` replaced by its new one.
pub(crate) fn text(text: &str, flows: &BTreeMap<String, String>) -> String {
    let mut text = text.to_owned();
    for (pattern, replacement) in TEXT.iter() {
        // A path may follow another directly, so each match's leading
        // character is left for the next.
        loop {
            let next = pattern.replace_all(&text, *replacement).into_owned();
            if next == text {
                break;
            }
            text = next;
        }
    }
    for (old, new) in flows {
        text = text.replace(old.as_str(), new);
    }
    text
}

/// `record`, of `record_type`, with its fields renamed and every string
/// rewritten by [`text`].
pub(crate) fn rewritten(
    record_type: Option<&str>,
    mut record: Value,
    flows: &BTreeMap<String, String>,
) -> Value {
    match record_type {
        Some(CONTROL_TYPE) => {
            if let Some(fields) = record.as_object_mut() {
                key(fields, "rule", "checker");
            }
        }
        Some("boundary/boundary/v1") => {
            for check in objects(record.get_mut("checks")) {
                key(check, "control", "rule");
            }
        }
        Some("boundary/crossing/v1") => {
            for verdict in objects(record.get_mut("verdicts")) {
                if verdict.contains_key("control") {
                    key(verdict, "rule", "checker");
                    key(verdict, "control", "rule");
                }
            }
        }
        Some("changes/plan/v1") => {
            for owed in objects(record.get_mut("owed")) {
                key(owed, "control", "rule");
            }
        }
        Some(FLOW_TYPE) => {
            let phases = objects(record.get_mut("phases"));
            for phase in phases {
                for check in objects(phase.get_mut("checks")) {
                    key(check, "controls", "rules");
                    if let Some(kind) = check.get_mut("kind")
                        && let Some((_, now)) = KINDS
                            .iter()
                            .find(|(former, _)| kind.as_str() == Some(former))
                    {
                        *kind = json!(now);
                    }
                }
            }
        }
        Some(CONFIG_TYPE) => {
            for plugin in record
                .get_mut("plugins")
                .and_then(Value::as_array_mut)
                .into_iter()
                .flatten()
            {
                if plugin.as_str() == Some("controls") {
                    *plugin = json!("rules");
                }
            }
        }
        _ => {}
    }
    if let Some((_, now)) = RENAMED.iter().find(|(old, _)| Some(*old) == record_type) {
        record["$type"] = json!(now);
    }
    strings_rewritten(record, flows)
}

/// The objects in the array `value` holds, if it holds one.
fn objects(value: Option<&mut Value>) -> Vec<&mut Map<String, Value>> {
    (value.and_then(Value::as_array_mut).into_iter().flatten())
        .filter_map(Value::as_object_mut)
        .collect()
}

/// `value` with every string rewritten by [`text`].
fn strings_rewritten(value: Value, flows: &BTreeMap<String, String>) -> Value {
    match value {
        Value::String(string) => Value::String(text(&string, flows)),
        Value::Array(each) => each
            .into_iter()
            .map(|one| strings_rewritten(one, flows))
            .collect(),
        Value::Object(fields) => (fields.into_iter())
            .map(|(name, one)| (name, strings_rewritten(one, flows)))
            .collect(),
        other => other,
    }
}

/// Renames `from` to `to` in `fields` when `to` is not there already.
fn key(fields: &mut Map<String, Value>, from: &str, to: &str) {
    if !fields.contains_key(to)
        && let Some(value) = fields.remove(from)
    {
        fields.insert(to.to_owned(), value);
    }
}

/// The SHA-256 of `lines` as canonical JSON, in hex.
fn digest_of(lines: &[Value]) -> String {
    let digest = Sha256::digest(canonical_json(&json!(lines)).as_str().as_bytes());
    digest.iter().fold(String::new(), |mut hex, byte| {
        let _ = write!(hex, "{byte:02x}");
        hex
    })
}

#[cfg(test)]
mod tests;
