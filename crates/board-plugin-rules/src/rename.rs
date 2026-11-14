//! `rules.migrate --rename true`: rewrites the board's own records from the
//! names before the rename to the names after it, history included, in one
//! commit with a `Board-Bulk:` trailer. The plugin's README says what moves,
//! what is renamed and what is kept.
//!
//! Signed records (attention orders and effects) and each order's context
//! are never rewritten. Only the board's own working tree is read and
//! written, never a mount. `--apply true` takes the dry run's digest and
//! refuses with `rules::migrate::stale`, writing nothing, when the writes
//! differ; each line also expects the blob it read.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use board_model::{
    CONFIG_TYPE, Digest, EFFECT_TYPES, LogicalPath, ORDER_TYPE, canonical_json, flow_version,
    parse_json,
};
use board_ops::{Bulk, Host, PluginError};
use regex::Regex;
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::migrate::{LISTED, Output, UNREAD};
use crate::records::{CONTROL_TYPE, RENAMED};

/// The reason the commit gives in its `Board-Bulk:` trailer.
const BULK: &str = "rename controls to rules";
const STALE: &str = "rules::migrate::stale";
const CONFIG_FIRST: &str = "rules::migrate::config_first";
const FLOW_TYPE: &str = "changes/flow/v1";
/// The board's config, which the rename never writes.
const CONFIG_PATH: &str = "/.board/board";
/// Each old field name in a record and the name it takes.
type Renames = &'static [(&'static str, &'static str)];
/// The records of the changes that rename controls to rules, which describe
/// the rename in the old names and so keep them.
const RENAMING: [&str; 2] = [
    "/changes/control-becomes-rule/",
    "/changes/rule-rename-rest/",
];
/// Each flow version that moves, with the version it becomes and the
/// record stored there.
type Flows = BTreeMap<String, (String, Value)>;
/// Each former check kind of a flow, and its name now.
const KINDS: [(&str, &str); 2] = [
    ("controlled-paths", "rule-paths"),
    ("planned-controlled", "planned-rule-paths"),
];
/// Each old name renamed wherever it appears, before [`PATHS`]: first
/// crumbkit/board's inner `rules/` folder, which its own tree dropped.
const WORDS: [(&str, &str); 3] = [
    ("/repos/board/controls/rules/", "/repos/board/rules/"),
    ("controls/control/v1", "rules/rule/v1"),
    ("controls/source/v1", "rules/source/v1"),
];

fn compiled(pattern: &str) -> Regex {
    Regex::new(pattern).expect("each pattern is a constant; the rename tests compile it")
}

/// Each rewrite of a path, in order: the inner `rules/` folder, a
/// `/controls/` path in this board or a mounted one, a capability and a
/// command.
static PATHS: LazyLock<[(Regex, &str); 4]> = LazyLock::new(|| {
    [
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
    ]
});

/// One write: the path rewritten, and where it moves when it moves. For a
/// flow version, `to` is where its new version is stored; the old version stays.
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
    why: &'static str,
}

/// One entry of the board's own tree, with its text, or `None` when it is
/// not UTF-8.
struct Entry {
    logical: String,
    blob: String,
    kind: String,
    role: Option<String>,
    record_type: Option<String>,
    text: Option<String>,
}

/// Fills `output` with the rename's writes and, with `apply`, writes them
/// when `digest` is the dry run's.
pub(crate) fn rename(
    host: &dyn Host,
    apply: bool,
    digest: Option<&str>,
    output: &mut Output,
) -> Result<(), PluginError> {
    let entries = entries(host)?;
    // Only the owner writes the config, so it names `rules` and no longer
    // `controls` before the rename. A binary that predates the rename then
    // refuses `controls.*` from the first commit after the apply, and a
    // config that names both would leave it reading an emptied `/controls/`.
    let config = (entries.iter()).find(|entry| entry.logical == CONFIG_PATH);
    if !config.is_some_and(|entry| names(entry, "rules") && !names(entry, "controls")) {
        return Err(refusal(
            CONFIG_FIRST,
            "the board's config does not name rules, or still names controls, in its plugins",
            "name rules in place of controls in .board/board.json, then run the rename again",
        ));
    }
    let mut held: BTreeSet<String> = (entries.iter())
        .map(|entry| entry.logical.clone())
        .collect();
    let (mut signed, mut kept) = (Vec::new(), Vec::new());
    let skipped = unsigned(&entries, &mut signed);
    let flows = flows(&entries, &mut kept);
    let (mut writes, mut lines) = (Vec::new(), Vec::new());
    for entry in &entries {
        if skipped.contains(&entry.logical)
            || entry.logical.starts_with("/.board/types/")
            || entry.kind == "schema"
        {
            continue;
        }
        let flow = flows.get(&entry.logical);
        // A flow rewritten in place has no new path.
        let to = (flow.map(|(new, _)| new.clone()))
            .filter(|new| *new != entry.logical)
            .or_else(|| moved(&entry.logical));
        let mut keep = |why| {
            kept.push(Kept {
                path: entry.logical.clone(),
                why,
            });
        };
        let record = flow.map(|(_, record)| record);
        let content = match content(entry, record, to.as_ref(), &flows) {
            Ok(Some(content)) => content,
            Ok(None) => continue,
            Err(why) => {
                keep(why);
                continue;
            }
        };
        let line = |path: &str, expect: &str| {
            let mut line = content.clone();
            line["path"] = json!(path);
            line["expect"] = json!(expect);
            line
        };
        match &to {
            // Two versions may rewrite to one, and the new version may be
            // stored already; it is stored once, and a version with no
            // line to write is not listed. Records naming the old version
            // are re-pinned either way.
            Some(to) if flow.is_some() && entry.logical.starts_with("/flows/") => {
                if !held.insert(to.clone()) {
                    continue;
                }
                lines.push(line(to, "none"));
            }
            Some(to) if held.contains(to) => {
                keep("its new path already holds a file; move or remove one by hand");
                continue;
            }
            Some(to) => lines.push(line(to, "none")),
            None => lines.push(line(&entry.logical, &entry.blob)),
        }
        // An old flow version stays: a change in a mounted board that this
        // migration cannot re-pin still names it.
        if to.is_some() && (flow.is_none() || !entry.logical.starts_with("/flows/")) {
            lines.push(json!({"path": entry.logical, "expect": entry.blob, "remove": true}));
        }
        writes.push(Write {
            path: entry.logical.clone(),
            to,
        });
    }
    let current = digest_of(&lines);
    if apply {
        write(host, &lines, digest == Some(current.as_str()))?;
    } else {
        // The refusal the apply would give, so a dry run that lists writes
        // is one whose apply the schemas admit.
        (host.check(&lines)).map_err(|error| refused_put(&error.refusal()))?;
    }
    output.total = writes.len();
    output.truncated = output.total > LISTED;
    writes.truncate(LISTED);
    output.writes = Some(writes);
    output.digest = Some(current);
    output.signed = Some(signed);
    output.kept = Some(kept);
    Ok(())
}

/// What a line writes for `entry`, which moves to `to`: the flow `flow` it
/// is stored again as, its rewritten record, or its rewritten text; none
/// when nothing changes. Why it is kept when only a person may change it.
fn content(
    entry: &Entry,
    flow: Option<&Value>,
    to: Option<&String>,
    flows: &Flows,
) -> Result<Option<Value>, &'static str> {
    let kept = |why| if to.is_some() { Err(why) } else { Ok(None) };
    let record_type = entry.record_type.as_deref();
    let (content, same) = if let Some(record) = flow {
        (json!({"record": record}), false)
    } else if entry.kind == "pointer" {
        return kept(
            "a mount pointer; move it to /rules/ with board put once its target board has migrated",
        );
    } else if matches!(record_type, Some(CONFIG_TYPE | FLOW_TYPE)) {
        // The config is the owner's to write; a flow is in `flows` or needs
        // no change.
        return Ok(None);
    } else if let Some(text) = &entry.text {
        if entry.role.as_deref() == Some("json") {
            let body = (parse_json(text.as_bytes()).ok())
                .filter(|body| entry.kind == "record" && body.is_object());
            let Some(body) = body else {
                return kept(
                    "its JSON does not parse as a record; fix it and run the rename again",
                );
            };
            let record = rewritten(record_type, body.clone(), flows);
            (json!({"record": record}), record == body)
        } else if entry.role.as_deref() == Some("markdown") || to.is_some() {
            let renamed = self::text(text, flows);
            (json!({"text": renamed}), renamed == *text)
        } else {
            return Ok(None);
        }
    } else {
        return kept(
            "it is not UTF-8 text, and a batch line moves only text; references to it are \
                      rewritten to its /rules/ path and dangle until it is moved by hand with git mv",
        );
    };
    if same && to.is_none() {
        return Ok(None);
    }
    if (RENAMING.iter()).any(|prefix| entry.logical.starts_with(prefix)) {
        return Err("it describes the rename itself, so it keeps the names it was written in");
    }
    Ok(Some(content))
}

/// Whether the config `entry` holds enables the plugin `plugin`.
fn names(entry: &Entry, plugin: &str) -> bool {
    let config = parse_json(entry.text.as_deref().unwrap_or_default().as_bytes());
    let plugins = config.unwrap_or_default()["plugins"].clone();
    (plugins.as_array()).is_some_and(|each| each.iter().any(|one| one == plugin))
}

/// Writes `lines` in one bulk commit, or refuses with
/// [`STALE`] when the digest given is not theirs.
fn write(host: &dyn Host, lines: &[Value], current: bool) -> Result<(), PluginError> {
    if !current {
        return Err(refusal(
            STALE,
            "the writes differ from those the dry run listed, so a record changed since it ran",
            "run rules.migrate --rename true again and apply with its digest",
        ));
    }
    if !lines.is_empty() {
        let bulk = Bulk::new(BULK).map_err(|error| refused_put(&error.refusal()))?;
        (host.put_bulk(lines, &bulk)).map_err(|error| refused_put(&error.refusal()))?;
    }
    Ok(())
}

/// Board's `refusal` of the batch the rename writes.
fn refused_put(refusal: &board_model::Refusal) -> PluginError {
    PluginError::board_refused("rules", "put --batch --bulk", refusal)
}

/// The rename's refusal `code`, saying that nothing was written.
fn refusal(code: &str, why: &str, hint: &str) -> PluginError {
    PluginError {
        code: code.to_owned(),
        message: format!("{why}; nothing was written"),
        fields: Map::new(),
        hint: Some(hint.to_owned()),
    }
}

/// The board's own records, Markdown files, pointers and files under
/// `/controls/`, each with its own text; refuses with [`UNREAD`] when one
/// cannot be read, so the rename never writes from a partial read.
fn entries(host: &dyn Host) -> Result<Vec<Entry>, PluginError> {
    let sql = "SELECT e.logical, e.blob, e.kind, e.role, e.type, b.body FROM entries AS e \
               LEFT JOIN bodies AS b ON b.blob = e.blob WHERE e.source = 'worktree' \
               AND e.kind IN ('record', 'json', 'file', 'pointer') ORDER BY e.logical";
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
                text: Some(text(row.get(5)).unwrap_or_default()),
            })
        })
        .filter(|entry| {
            !matches!(entry.kind.as_str(), "file" | "json")
                || (entry.kind == "file" && entry.role.as_deref() == Some("markdown"))
                || moved(&entry.logical).is_some()
        })
        .collect();
    let unread = |path: &str, why: &str| {
        let why = format!("Board could not read {path}: {why}");
        refusal(
            UNREAD,
            &why,
            "fix or remove the file, then run the rename again",
        )
    };
    let files: Vec<usize> = (0..entries.len())
        .filter(|at| entries[*at].kind != "pointer" && entries[*at].role.as_deref() != Some("json"))
        .collect();
    let paths = (files.iter())
        .map(|at| (entries[*at].logical.parse::<LogicalPath>()).map_err(|_| *at))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|at| unread(&entries[at].logical, "its path is not a logical path"))?;
    let read = (host.get_each(&paths))
        .map_err(|error| PluginError::board_refused("rules", "get", &error.refusal("get")))?;
    for (at, got) in files.into_iter().zip(read) {
        let got =
            got.map_err(|error| unread(&entries[at].logical, &error.refusal("get").message))?;
        entries[at].text = String::from_utf8(got.bytes).ok();
    }
    Ok(entries)
}

/// The paths the rename never rewrites: each signed record and each
/// order's context. Each signed record that holds an old name goes in
/// `listed`.
fn unsigned(entries: &[Entry], listed: &mut Vec<String>) -> BTreeSet<String> {
    let mut skipped = BTreeSet::new();
    for entry in entries {
        let record_type = entry.record_type.as_deref().unwrap_or_default();
        if record_type != ORDER_TYPE && !EFFECT_TYPES.contains(&record_type) {
            continue;
        }
        skipped.insert(entry.logical.clone());
        let body = entry.text.as_deref().unwrap_or_default();
        if text(body, &Flows::new()) != body {
            listed.push(entry.logical.clone());
        }
        if let Ok(record) = parse_json(body.as_bytes())
            && let Some(context) = record.get("context").and_then(Value::as_str)
            && record_type == ORDER_TYPE
        {
            skipped.insert(context.to_owned());
        }
    }
    skipped
}

/// Each flow version stored at the hash of its JSON whose rewritten JSON
/// has another hash, with that hash's path and the record written there.
/// A version that would change but is not stored at its hash goes in
/// `kept`.
fn flows(entries: &[Entry], kept: &mut Vec<Kept>) -> Flows {
    let mut flows = BTreeMap::new();
    for entry in entries {
        let text = entry.text.as_deref().unwrap_or_default();
        let parsed = (entry.record_type.as_deref() == Some(FLOW_TYPE))
            .then(|| parse_json(text.as_bytes()).ok());
        let Some(Some(value)) = parsed else {
            continue;
        };
        let record = rewritten(Some(FLOW_TYPE), value.clone(), &Flows::new());
        let name = value["name"].as_str().unwrap_or_default();
        let (stored, at_hash) = (flow_version(name, &record), flow_version(name, &value));
        if at_hash == entry.logical && stored != entry.logical {
            flows.insert(entry.logical.clone(), (stored, record));
        } else if at_hash != entry.logical && (record != value || moved(&entry.logical).is_some()) {
            // A flow outside /flows/, such as one a change holds, is pinned
            // by its path, not its hash, so every such flow off its hash is
            // rewritten in place, or moved with the other files under
            // /controls/. One under /flows/ off its hash is a version a
            // change may pin by a hash that is not its own.
            if entry.logical.starts_with("/flows/") {
                kept.push(Kept {
                    path: entry.logical.clone(),
                    why: "a flow not stored at the hash of its JSON; store its new version with \
                          the changes plugin and re-pin its changes by hand",
                });
            } else {
                let new = moved(&entry.logical).unwrap_or_else(|| entry.logical.clone());
                flows.insert(entry.logical.clone(), (new, record));
            }
        }
    }
    flows
}

/// Where a path under `/controls/` moves: the same path under `/rules/`.
fn moved(path: &str) -> Option<String> {
    path.strip_prefix("/controls/")
        .map(|rest| format!("/rules/{rest}"))
}

/// `text` with every old path, capability and type renamed, and every flow
/// version in `flows` replaced by its new one.
pub(crate) fn text(text: &str, flows: &Flows) -> String {
    let mut text = text.to_owned();
    for (old, new) in WORDS {
        text = text.replace(old, new);
    }
    for (pattern, replacement) in PATHS.iter() {
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
    for (old, (new, _)) in flows {
        text = text.replace(old.as_str(), new);
    }
    text
}

/// `record`, of `record_type`, with its fields renamed and every string
/// rewritten by [`text`].
pub(crate) fn rewritten(record_type: Option<&str>, mut record: Value, flows: &Flows) -> Value {
    let (holders, renames): (Vec<_>, Renames) = match record_type {
        Some(CONTROL_TYPE) => (
            record.as_object_mut().into_iter().collect(),
            &[("rule", "checker")],
        ),
        Some("boundary/boundary/v1") => (objects(record.get_mut("checks")), &[("control", "rule")]),
        // A verdict's kind moves from `rule` to `checker` before its path
        // takes `rule`.
        Some("boundary/crossing/v1") => (
            objects(record.get_mut("verdicts")),
            &[("rule", "checker"), ("control", "rule")],
        ),
        Some("changes/plan/v1") => (objects(record.get_mut("owed")), &[("control", "rule")]),
        Some(FLOW_TYPE) => (
            (objects(record.get_mut("phases")).into_iter())
                .flat_map(|phase| objects(phase.get_mut("checks")))
                .collect(),
            &[("controls", "rules")],
        ),
        _ => (Vec::new(), &[]),
    };
    for holder in holders {
        // Only a holder with the last old name is renamed, so a verdict
        // already in the new names keeps them.
        let old = renames
            .last()
            .is_some_and(|(from, _)| holder.contains_key(*from));
        for (from, to) in renames.iter().filter(|_| old) {
            if !holder.contains_key(*to)
                && let Some(value) = holder.remove(*from)
            {
                holder.insert((*to).to_owned(), value);
            }
        }
        if let Some(kind) = holder.get_mut("kind")
            && let Some((_, now)) = KINDS
                .iter()
                .find(|(former, _)| kind.as_str() == Some(former))
            && record_type == Some(FLOW_TYPE)
        {
            *kind = json!(now);
        }
    }
    if let Some((_, now)) = RENAMED.iter().find(|(old, _)| Some(*old) == record_type) {
        record["$type"] = json!(now);
    }
    strings_rewritten(&mut record, flows);
    record
}

/// The objects in the array `value` holds, if it holds one.
fn objects(value: Option<&mut Value>) -> Vec<&mut Map<String, Value>> {
    (value.and_then(Value::as_array_mut).into_iter().flatten())
        .filter_map(Value::as_object_mut)
        .collect()
}

/// Rewrites every string in `value` by [`text`].
fn strings_rewritten(value: &mut Value, flows: &Flows) {
    match value {
        Value::String(string) => *string = text(string, flows),
        Value::Array(each) => each
            .iter_mut()
            .for_each(|one| strings_rewritten(one, flows)),
        Value::Object(fields) => fields
            .values_mut()
            .for_each(|one| strings_rewritten(one, flows)),
        _ => {}
    }
}

/// The SHA-256 of `lines` as canonical JSON, in hex.
fn digest_of(lines: &[Value]) -> String {
    Digest::of(canonical_json(&json!(lines)).as_str().as_bytes()).hex()
}

#[cfg(test)]
mod tests;
