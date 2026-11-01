//! What the plugin reads from the index: its controls and the files their
//! patterns match. Matching stays in `SQLite`, with its GLOB.

use std::collections::BTreeMap;

use board_model::{LogicalPath, parse_json};
use board_ops::{Host, Limit, PluginError, QueryReport};
use serde::Deserialize;
use serde_json::Value;

use crate::records::{Budget, Kind, Show};

/// Each file a pattern can match, with `full`: its display path without the
/// leading `/`, with `.json` put back when the display path dropped it. A
/// file in the own tree whose path is malformed has no display path and
/// keeps its path, so a budget never leaves it out silently. A query names
/// it with `files AS {FILES}`, and adds MATERIALIZED where it compares every
/// file with every pattern, so each row is computed once. Without
/// MATERIALIZED, a lookup of one file uses the index on its source and path.
pub(crate) const FILES: &str = "(SELECT *, COALESCE(substr(logical, 2) \
    || CASE WHEN substr(logical, -length(path) - 1) = '/' || path THEN '' ELSE '.json' END, \
    CASE source WHEN 'worktree' THEN path END) AS full FROM entries WHERE kind != 'pointer')";

/// Each pattern of each readable control in the board's own tree.
/// MATERIALIZED parses each control's body once, not once for every file a
/// pattern is compared with.
const PATTERNS: &str = "patterns AS MATERIALIZED (SELECT r.logical, r.path, p.value AS pattern \
    FROM records r, json_each(r.body, '$.paths') p \
    WHERE r.type = 'controls/control/v1' AND r.status = 'ok' AND r.source = 'worktree')";

/// Every valid control in the board's own tree. That includes one Board
/// marks degraded only because a reference dangles or reaches the wrong
/// type, which Board checks only of a valid record.
const CONTROLS: &str = "SELECT logical, body FROM records \
    WHERE type = 'controls/control/v1' AND source = 'worktree' \
    AND (status = 'ok' OR code IN ('board::ref::dangling', 'board::ref::wrong_type'))";

/// Each control in the board's own tree that Board marks degraded, by its
/// logical path, or by its path when it has none.
const DEGRADED: &str = "SELECT COALESCE(logical, path), code FROM records \
    WHERE type = 'controls/control/v1' AND status != 'ok' AND source = 'worktree' ORDER BY logical";

/// A control as the board's own copy of the schema admits it, which equals
/// the shipped one, or Board refuses the call.
#[derive(Deserialize)]
pub(crate) struct Control {
    pub(crate) requirement: String,
    pub(crate) purpose: String,
    pub(crate) applies_to: Option<Vec<String>>,
    pub(crate) paths: Option<Vec<String>>,
    pub(crate) rule: Option<Budget>,
    #[serde(default)]
    pub(crate) kind: Kind,
    pub(crate) show: Option<Show>,
}

impl Control {
    /// When the mod shows this control, its default applied.
    pub(crate) fn show(&self) -> Show {
        Show::effective(self.show, self.kind, self.rule.is_some())
    }
}

/// One file a control's pattern matches, and whether Board can read it.
pub(crate) struct Match {
    pub(crate) control: String,
    pub(crate) file: File,
    pub(crate) health: Health,
}

/// A matched file by its display path, or, for a file in the board's own
/// tree whose path is malformed and which so has none, by its path from the
/// board's root. Board marks such a file degraded.
pub(crate) enum File {
    Named(LogicalPath),
    Unnamed(String),
}

/// Whether Board reads a file whole, or marks it degraded with a code.
pub(crate) enum Health {
    Whole,
    Degraded(Option<String>),
}

/// SQL `text` as a string literal. Board's query takes no parameters.
pub(crate) fn literal(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

/// The rows of `sql`, every one of them.
pub(crate) fn rows(host: &dyn Host, sql: &str) -> Result<QueryReport, PluginError> {
    (host.query(sql, Limit::ALL))
        .map_err(|error| PluginError::board_refused("controls", "query", &error.refusal()))
}

/// Every valid control in the board's own tree, by its logical path.
pub(crate) fn controls(host: &dyn Host) -> Result<BTreeMap<String, Control>, PluginError> {
    let report = rows(host, CONTROLS)?;
    Ok((report.rows.iter())
        .filter_map(|row| {
            let [Value::String(logical), Value::String(body)] = &row[..] else {
                return None;
            };
            let record = parse_json(body.as_bytes()).ok()?;
            let control = Control::deserialize(&record).ok()?;
            Some((logical.clone(), control))
        })
        .collect())
}

/// Each control in the board's own tree that Board marks degraded, with
/// its code, by path.
pub(crate) fn degraded(host: &dyn Host) -> Result<Vec<(String, Value)>, PluginError> {
    let report = rows(host, DEGRADED)?;
    Ok((report.rows.into_iter())
        .filter_map(|row| match <[Value; 2]>::try_from(row) {
            Ok([Value::String(logical), code]) => Some((logical, code)),
            _ => None,
        })
        .collect())
}

/// Each control and each file one of its patterns matches. A pattern
/// matches a file's `full` path (see [`FILES`]), so files in mounted boards
/// match by their display path through the mount. A pointer's file holds
/// where it leads, never what a budget counts, so no pattern matches one.
pub(crate) fn matched(host: &dyn Host) -> Result<Vec<Match>, PluginError> {
    let sql = format!(
        "WITH files AS MATERIALIZED {FILES}, {PATTERNS} SELECT DISTINCT \
         c.logical, COALESCE(e.logical, e.full), e.status, e.code, c.path, e.full, e.source \
         FROM patterns c, files e WHERE e.full GLOB c.pattern \
         ORDER BY c.path, e.full, e.source"
    );
    let report = rows(host, &sql)?;
    Ok((report.rows.into_iter())
        .filter_map(|row| {
            let mut row = row.into_iter();
            let (Some(Value::String(control)), Some(Value::String(file)), Some(status)) =
                (row.next(), row.next(), row.next())
            else {
                return None;
            };
            let code = row.next().and_then(|code| code.as_str().map(str::to_owned));
            let health = if status == "ok" {
                Health::Whole
            } else {
                Health::Degraded(code)
            };
            let file = match file.parse() {
                Ok(path) => File::Named(path),
                Err(_) => File::Unnamed(file),
            };
            Some(Match {
                control,
                file,
                health,
            })
        })
        .collect())
}
