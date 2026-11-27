//! What the plugin reads from the index: its controls and the files their
//! patterns match. `check` matches its patterns in Rust (see [`matched`]);
//! the other capabilities match theirs in `SQLite`, with its GLOB.

use std::collections::{BTreeMap, BTreeSet};

use board_model::{LogicalPath, parse_json};
use board_ops::{Host, Limit, PluginError, QueryReport, literal};
use board_plugin::shape::Open;
use serde::Deserialize;
use serde_json::Value;

use crate::glob;
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

/// Whether Board reads a record as valid, as an SQL condition: it marks
/// it ok, or degraded only because a reference dangles, reaches the wrong
/// type or reaches through a mount, which Board checks only of a valid
/// record. A macro, so `concat!` builds queries with it.
macro_rules! readable {
    () => {
        "(status = 'ok' OR code IN ('board::ref::dangling', 'board::ref::wrong_type', \
         'board::ref::through_mount'))"
    };
}
pub(crate) use readable;

/// Every valid control in the board's own tree, as [`readable`] reads it.
const CONTROLS: &str = concat!(
    "SELECT logical, body FROM records WHERE type IN ",
    rule_types!(),
    " AND source = 'worktree' AND ",
    readable!()
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
    pub(crate) judged_by: Option<String>,
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

    /// Whether `check` measures the control: it has a budget, version or
    /// zero rule this Board can apply.
    pub(crate) fn measured(&self) -> bool {
        self.reads_files() || matches!(self.checker, Some(Checker::Zero(_)))
    }

    /// Whether `check` measures the control's files: it has a budget or a
    /// version rule this Board can apply. A zero rule reads a suite's runs.
    pub(crate) fn reads_files(&self) -> bool {
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
        .map_err(|error| PluginError::board_refused("rules", "query", &error.refusal()))
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

/// Each of `controls` and each file one of its patterns matches, of the
/// files at or under `scope`, ordered by control and then by each file's
/// `full` path and source, with a file that several of a control's
/// patterns match listed once. A pattern matches a file's `full` path (see
/// [`FILES`]) as `SQLite`'s GLOB would, so files in mounted boards match by
/// their display path through the mount. A pointer's file holds where it
/// leads, never what a budget counts, so no pattern matches one. Under `/`
/// every file is listed; under another path only the files there are. A
/// file with no display path lies under `/` alone.
///
/// One query lists the files and Rust compares them with the patterns,
/// because comparing every file with every pattern in `SQLite` passes the
/// query time limit on a board that mounts many repositories. The files are
/// sorted by `full`, so each pattern is compared only with the files that
/// start with its literal prefix.
pub(crate) fn matched(
    host: &dyn Host,
    scope: &LogicalPath,
    controls: &BTreeMap<String, Control>,
) -> Result<Vec<Match>, PluginError> {
    let within = if *scope == LogicalPath::root() {
        String::new()
    } else {
        let exact = literal(scope.as_str());
        let beneath = literal(&format!("{}/", scope.as_str()));
        format!(" AND (logical = {exact} OR substr(logical, 1, length({beneath})) = {beneath})")
    };
    let sql = format!(
        "SELECT COALESCE(logical, full), status, code, full, source FROM {FILES} \
         WHERE full IS NOT NULL{within}"
    );
    let mut files: Vec<Listed> = (rows(host, &sql)?.rows.into_iter())
        .filter_map(|row| match <[Value; 5]>::try_from(row) {
            Ok(
                [
                    Value::String(shown),
                    status,
                    code,
                    Value::String(full),
                    source,
                ],
            ) => Some(Listed {
                shown,
                whole: status == "ok",
                code: code.as_str().map(str::to_owned),
                full,
                source: source.as_str().map(str::to_owned),
            }),
            _ => None,
        })
        .collect();
    files.sort_by(|one, other| (&one.full, &one.source).cmp(&(&other.full, &other.source)));
    let mut found = Vec::new();
    for (logical, control) in controls {
        let mut reached = BTreeSet::new();
        for pattern in control.paths.iter().flatten() {
            let prefix = glob::prefix(pattern);
            let first = files.partition_point(|file| file.full.as_str() < prefix);
            let candidates = (files[first..].iter().enumerate())
                .take_while(|(_, file)| file.full.starts_with(prefix));
            for (at, file) in candidates {
                if glob::glob(pattern, &file.full) {
                    reached.insert(first + at);
                }
            }
        }
        found.extend(reached.into_iter().map(|at| {
            let file = &files[at];
            Match {
                control: logical.clone(),
                file: match file.shown.parse() {
                    Ok(path) => File::Named(path),
                    Err(_) => File::Unnamed(file.shown.clone()),
                },
                health: if file.whole {
                    Health::Whole
                } else {
                    Health::Degraded(file.code.clone())
                },
            }
        }));
    }
    Ok(found)
}

/// A file as [`matched`] lists it: by its display path or, with none, its
/// `full` path, whether Board reads it whole, its code when not, its
/// `full` path and its source.
struct Listed {
    shown: String,
    whole: bool,
    code: Option<String>,
    full: String,
    source: Option<String>,
}
