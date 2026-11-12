//! The rules that bear on a change, read through the rules plugin by a
//! nested call and from the board's index: the obligations on a plan's
//! paths or a mount, and the files of a diff that a named rule lists.
//!
//! A board that has not run `rules.migrate --rename` keeps its rules under
//! `/controls/`, and a flow stored before the rename names them there, so
//! each rule's path is compared in its `/rules/` form and looked up in both.

use board_ops::{Host, PluginError, literal};
use serde_json::{Value, json};

use crate::board::{body, call_or_refusal, readable, refused, rows, text};

/// The record types of a rule, the second from before the rename.
pub(crate) const RULE_TYPES: [&str; 2] = ["rules/rule/v1", "controls/control/v1"];
/// The prefix rules moved from, and the prefix they moved to.
const MOVED: (&str, &str) = ("/controls/", "/rules/");
/// The codes Board gives a call to a plugin, or a capability, it does not offer.
pub(crate) const RULES_UNAVAILABLE: [&str; 3] = [
    "board::plugin::not_installed",
    "board::plugin::no_capability",
    "board::plugin::not_enabled",
];

/// `path` in its `/rules/` form: a path under `/controls/` moves there, and
/// any other path is kept.
pub(crate) fn current(path: &str) -> String {
    (path.strip_prefix(MOVED.0))
        .map_or_else(|| path.to_owned(), |rest| format!("{}{rest}", MOVED.1))
}

/// Both paths a rule at `path` may be stored at, quoted for SQL.
fn stored_at(path: &str) -> [String; 2] {
    let now = current(path);
    let before = (now.strip_prefix(MOVED.1))
        .map_or_else(|| now.clone(), |rest| format!("{}{rest}", MOVED.0));
    [literal(&now), literal(&before)]
}

/// The rule types, quoted for SQL's `IN`.
fn rule_types() -> String {
    RULE_TYPES.map(literal).join(", ")
}

/// A rule `rules.for-paths` found: its path where it is stored, its
/// summary and, for an obligation, the line that says how to close it, as
/// the rules plugin gives it. An agent reads and closes the rule at that
/// path, so it stays under `/controls/` until the board is migrated.
#[derive(Debug)]
pub(crate) struct Found {
    pub(crate) rule: String,
    pub(crate) summary: String,
    pub(crate) close: Option<String>,
}

/// The rules `rules.for-paths` finds on `paths`; none when the rules
/// plugin is not installed or not enabled, since a board without it records
/// no obligations.
fn for_paths(host: &dyn Host, paths: &[String]) -> Result<Vec<Found>, PluginError> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    let output = match call_or_refusal(host, "rules.for-paths", &json!({"paths": paths})) {
        Ok(output) => output,
        Err(refusal) if RULES_UNAVAILABLE.contains(&refusal.code.as_str()) => {
            return Ok(Vec::new());
        }
        Err(refusal) => return Err(refused("call", &refusal)),
    };
    Ok((output
        .get("controls")
        .and_then(Value::as_array)
        .into_iter()
        .flatten())
    .filter_map(|rule| {
        Some(Found {
            rule: rule.get("control")?.as_str()?.to_owned(),
            summary: rule.get("summary")?.as_str()?.to_owned(),
            close: (rule.get("close").and_then(Value::as_str)).map(str::to_owned),
        })
    })
    .collect())
}

/// Every rule on `paths`, a plan's files and patterns, by its path in its
/// `/rules/` form; none when the rules plugin is not enabled.
pub(crate) fn on_paths(host: &dyn Host, paths: &[String]) -> Result<Vec<String>, PluginError> {
    Ok((for_paths(host, paths)?.iter())
        .map(|found| current(&found.rule))
        .collect())
}

/// Which of `rules` are obligations, by the `kind` their records hold.
fn obligations_among(host: &dyn Host, rules: &[String]) -> Result<Vec<String>, PluginError> {
    if rules.is_empty() {
        return Ok(Vec::new());
    }
    let listed: Vec<String> = rules.iter().flat_map(|each| stored_at(each)).collect();
    let sql = format!(
        "SELECT logical FROM records WHERE type IN ({}) AND logical IN ({}) AND \
         json_extract(body, '$.kind') = 'obligation' ORDER BY logical",
        rule_types(),
        listed.join(", ")
    );
    let mut owed: Vec<String> = (rows(host, &sql)?.iter())
        .filter_map(|row| text(row.first()).map(current))
        .collect();
    owed.sort();
    owed.dedup();
    Ok(owed)
}

/// The obligations on `paths`, a plan's files and patterns, each by the path
/// it is stored at; none when the rules plugin is not enabled.
pub(crate) fn obligations(host: &dyn Host, paths: &[String]) -> Result<Vec<String>, PluginError> {
    Ok(obligations_in(host, for_paths(host, paths)?)?
        .into_iter()
        .map(|found| found.rule)
        .collect())
}

/// Whether a rule of either type that Board reads is stored at `path` under
/// either prefix.
pub(crate) fn exists(host: &dyn Host, path: &str) -> Result<bool, PluginError> {
    let sql = format!(
        "SELECT code FROM records WHERE type IN ({}) AND logical IN ({})",
        rule_types(),
        stored_at(path).join(", ")
    );
    Ok(rows(host, &sql)?.iter().any(|row| readable(row.first())))
}

/// The obligations on anything under `mount`, by path, with each one's
/// summary and close line; none when the rules plugin is not enabled.
pub(crate) fn on_mount(host: &dyn Host, mount: &str) -> Result<Vec<Found>, PluginError> {
    let pattern = format!("{}/**", mount.trim_end_matches('/'));
    obligations_in(host, for_paths(host, &[pattern])?)
}

/// The obligations among the rules `found`.
fn obligations_in(host: &dyn Host, found: Vec<Found>) -> Result<Vec<Found>, PluginError> {
    let rules: Vec<String> = found.iter().map(|each| current(&each.rule)).collect();
    let owed = obligations_among(host, &rules)?;
    Ok(found
        .into_iter()
        .filter(|each| owed.contains(&current(&each.rule)))
        .collect())
}

/// The files of `edited`, by logical path, that the rule at `rule` lists in
/// its `paths` or `applies_to`, or none when the board holds no rule there,
/// under either prefix, that Board reads.
pub(crate) fn listed_by(
    host: &dyn Host,
    rule: &str,
    edited: &[String],
) -> Result<Option<Vec<String>>, PluginError> {
    // `/rules/` sorts after `/controls/`, so a migrated rule wins.
    let sql = format!(
        "SELECT code, body FROM records WHERE logical IN ({}) AND type IN ({}) ORDER BY \
         logical DESC",
        stored_at(rule).join(", "),
        rule_types()
    );
    let found = rows(host, &sql)?;
    let Some(row) = found.first().filter(|row| readable(row.first())) else {
        return Ok(None);
    };
    let record = body(row.get(1));
    let list = |key: &str| -> Vec<String> {
        (record
            .get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten())
        .filter_map(|each| each.as_str().map(str::to_owned))
        .collect()
    };
    let (patterns, named) = (list("paths"), list("applies_to"));
    if edited.is_empty() {
        return Ok(Some(Vec::new()));
    }
    // A rule's paths are GLOB patterns on the path without its leading
    // slash, as the rules plugin matches them.
    let bare: Vec<&str> = edited
        .iter()
        .map(|each| each.trim_start_matches('/'))
        .collect();
    let listed = |each: &Value| literal(&each.to_string());
    let sql = format!(
        "SELECT '/' || f.value FROM json_each({}) AS f WHERE EXISTS (SELECT 1 FROM json_each({}) \
         AS p WHERE f.value GLOB p.value) ORDER BY f.key",
        listed(&json!(bare)),
        listed(&json!(patterns))
    );
    let mut touched: Vec<String> = (rows(host, &sql)?.iter())
        .filter_map(|row| text(row.first()).map(str::to_owned))
        .collect();
    for each in edited {
        if named.contains(each) && !touched.contains(each) {
            touched.push(each.clone());
        }
    }
    Ok(Some(touched))
}
