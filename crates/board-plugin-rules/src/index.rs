//! What the plugin reads from the index: its controls and the files their
//! patterns match. Matching stays in `SQLite`, with its GLOB.

use std::collections::BTreeMap;

use board_model::{LogicalPath, parse_json};
use board_ops::{Host, Limit, PluginError, QueryReport, literal};
use board_plugin::shape::Open;
use serde::Deserialize;
use serde_json::Value;

use crate::records::{Checker, Kind, ShowAt, rule_types};

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
const PATTERNS: &str = concat!(
    "patterns AS MATERIALIZED (SELECT r.logical, r.path, p.value AS pattern \
     FROM records r, json_each(r.body, '$.paths') p WHERE r.type IN ",
    rule_types!(),
    " AND r.status = 'ok' AND r.source = 'worktree')"
);

/// The first two columns of each row of `sql` that holds a string in both.
pub(crate) fn string_pairs<C: FromIterator<(String, String)>>(
    host: &dyn Host,
    sql: &str,
) -> Result<C, PluginError> {
    Ok((rows(host, sql)?.rows.into_iter())
        .filter_map(|row| match <[Value; 2]>::try_from(row) {
            Ok([Value::String(first), Value::String(second)]) => Some((first, second)),
            _ => None,
        })
        .collect())
}

/// Every valid control in the board's own tree. That includes one Board
/// marks degraded only because a reference dangles or reaches the wrong
/// type, which Board checks only of a valid record.
const CONTROLS: &str = concat!(
    "SELECT logical, body FROM records WHERE type IN ",
    rule_types!(),
    " AND source = 'worktree' \
     AND (status = 'ok' OR code IN ('board::ref::dangling', 'board::ref::wrong_type'))"
);

/// Each control in the board's own tree that Board marks degraded, by its
/// logical path, or by its path when it has none.
const DEGRADED: &str = concat!(
    "SELECT COALESCE(logical, path), code FROM records WHERE type IN ",
    rule_types!(),
    " AND status != 'ok' AND source = 'worktree' ORDER BY logical"
);

/// A rule as the board's own copy of the schema admits it, which equals
/// the shipped one, or Board refuses the call. A rule of the type before
/// the rename names its checker `rule`.
#[derive(Deserialize)]
pub(crate) struct Control {
    pub(crate) requirement: String,
    pub(crate) purpose: String,
    pub(crate) summary: Option<String>,
    pub(crate) applies_to: Option<Vec<String>>,
    pub(crate) paths: Option<Vec<String>>,
    #[serde(alias = "rule")]
    pub(crate) checker: Option<Checker>,
    kind: Option<Open<Kind>>,
    show: Option<Open<ShowAt>>,
    owed_by: Option<String>,
}

impl Control {
    /// The control's kind, `standing` when left out or when it holds a
    /// value this Board does not know, such as one an edited copy of the
    /// schema admits.
    pub(crate) fn kind(&self) -> Kind {
        (self.kind.as_ref().and_then(Open::known).copied()).unwrap_or_default()
    }

    /// When the mod shows this control, its default applied. A value this
    /// Board does not know, such as one an edited copy of the schema
    /// admits, reads as `start`, the first point it is shown.
    pub(crate) fn show(&self) -> ShowAt {
        let show = (self.show.as_ref()).map(|show| show.known().copied().unwrap_or(ShowAt::Start));
        ShowAt::effective(show, self.kind(), self.measured())
    }

    /// What left an obligation owed; none for a standing control, whose
    /// `owed_by` `check` warns of.
    pub(crate) fn owed(&self) -> Option<String> {
        (self.kind() == Kind::Obligation)
            .then(|| self.owed_by.clone())
            .flatten()
    }

    /// Whether the control is standing and yet names what left it owed.
    pub(crate) fn owed_though_standing(&self) -> bool {
        self.kind() == Kind::Standing && self.owed_by.is_some()
    }

    /// Whether `check` measures the control's files: it has a budget or a
    /// version rule this Board can apply.
    pub(crate) fn measured(&self) -> bool {
        (self.checker.as_ref()).is_some_and(|checker| {
            matches!(checker, Checker::Budget(_) | Checker::Version(_)) && checker.applies()
        })
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

/// The rows of `sql`, every one of them.
pub(crate) fn rows(host: &dyn Host, sql: &str) -> Result<QueryReport, PluginError> {
    (host.query(sql, Limit::ALL))
        .map_err(|error| PluginError::board_refused("controls", "query", &error.refusal()))
}

/// The first column of each row of `sql` that holds a string there.
pub(crate) fn first_strings<C: FromIterator<String>>(
    host: &dyn Host,
    sql: &str,
) -> Result<C, PluginError> {
    Ok((rows(host, sql)?.rows.into_iter())
        .filter_map(|row| match row.into_iter().next() {
            Some(Value::String(text)) => Some(text),
            _ => None,
        })
        .collect())
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

/// Each control and each file one of its patterns matches, of the files
/// at or under `scope`. A pattern matches a file's `full` path (see
/// [`FILES`]), so files in mounted boards match by their display path
/// through the mount. A pointer's file holds where it leads, never what a
/// budget counts, so no pattern matches one. Under `/` every file is
/// compared; under another path only the files there are, which keeps the
/// comparison of every file with every pattern small in a board that
/// mounts many repositories. A file with no display path lies under `/`
/// alone.
pub(crate) fn matched(host: &dyn Host, scope: &LogicalPath) -> Result<Vec<Match>, PluginError> {
    let within = if *scope == LogicalPath::root() {
        String::new()
    } else {
        let exact = literal(scope.as_str());
        let beneath = literal(&format!("{}/", scope.as_str()));
        format!(" WHERE logical = {exact} OR substr(logical, 1, length({beneath})) = {beneath}")
    };
    let sql = format!(
        "WITH files AS MATERIALIZED (SELECT * FROM {FILES}{within}), {PATTERNS} SELECT DISTINCT \
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
