//! The boundaries in the personal board, and which of them cover content.
//! Globs match with `SQLite`'s GLOB, in one query per call.

use std::collections::{BTreeMap, BTreeSet};

use board_model::parse_json;
use board_ops::{Host, Limit, PluginError, literal};
use board_plugin::shape::{Record, TREE_CODES};
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::records::BoundaryRecord;
use crate::refused;

/// Why an entry under `/boundaries/` is not a boundary, and the step that
/// fixes it.
const NOT_A_BOUNDARY: &str = "not a boundary";
const MOVE_OUT: &str = "move each entry that is not a boundary out of /boundaries/";
/// Why `/boundaries` itself is refused, and the step that fixes it.
const NOT_A_DIRECTORY: &str = "is not a directory";
const MAKE_DIRECTORY: &str = "make /boundaries/ a directory, not a symlink or a file";

/// A valid record: one Board marks ok, or degraded only for a reference
/// that dangles or reaches the wrong type.
pub(crate) fn valid() -> String {
    let codes: Vec<String> = TREE_CODES.iter().map(|code| literal(code)).collect();
    format!("(status = 'ok' OR code IN ({}))", codes.join(", "))
}

/// A boundary and its path in the personal board.
pub(crate) struct Boundary {
    pub(crate) path: String,
    pub(crate) record: BoundaryRecord,
}

impl Boundary {
    /// Whether it covers `action` to `to` from `from`. A boundary limited by
    /// `from` also covers a check that names no checkout. `from` is compared
    /// as path text, at or under each path, so a checkout named through a
    /// link to its mount is not matched (a soft limit; `board refs`
    /// resolving both paths would match it).
    pub(crate) fn covers(&self, globs: &Globs, action: &str, to: &str, from: Option<&str>) -> bool {
        let record = &self.record;
        let within = |path: &String| {
            from.is_none_or(|from| {
                from == path || from.starts_with(&format!("{}/", path.trim_end_matches('/')))
            })
        };
        globs.matches(&to.to_lowercase(), &record.to.to_lowercase())
            && (record.actions.iter()).any(|glob| globs.matches(action, glob))
            && (record.from.as_ref()).is_none_or(|paths| paths.iter().any(within))
    }

    /// The text and glob pairs that [`Boundary::covers`] and its checks of
    /// `parts` ask about.
    pub(crate) fn asked(&self, action: &str, to: &str, parts: &[&String]) -> Vec<(String, String)> {
        let record = &self.record;
        let mut asked = vec![(to.to_lowercase(), record.to.to_lowercase())];
        asked.extend((record.actions.iter()).map(|glob| (action.to_owned(), glob.clone())));
        for glob in record.checks.iter().flat_map(|check| &check.parts) {
            asked.extend(parts.iter().map(|part| ((*part).clone(), glob.clone())));
        }
        asked
    }
}

/// Whether each text matches each glob it was asked against, answered by
/// one GLOB query.
pub(crate) struct Globs(BTreeSet<(String, String)>);

impl Globs {
    pub(crate) fn answer(
        host: &dyn Host,
        asked: impl IntoIterator<Item = (String, String)>,
    ) -> Result<Self, PluginError> {
        let asked: BTreeSet<(String, String)> = asked.into_iter().collect();
        if asked.is_empty() {
            return Ok(Self(asked));
        }
        let values: Vec<String> = (asked.iter())
            .map(|(text, glob)| format!("({}, {})", literal(text), literal(glob)))
            .collect();
        let sql = format!(
            "SELECT column1, column2 FROM (VALUES {}) WHERE column1 GLOB column2",
            values.join(", ")
        );
        let rows = rows(host, &sql)?;
        Ok(Self(
            (rows.into_iter())
                .filter_map(|row| match <[Value; 2]>::try_from(row) {
                    Ok([Value::String(text), Value::String(glob)]) => Some((text, glob)),
                    _ => None,
                })
                .collect(),
        ))
    }

    pub(crate) fn matches(&self, text: &str, glob: &str) -> bool {
        self.0.contains(&(text.to_owned(), glob.to_owned()))
    }
}

/// Every boundary in the personal board's own tree, by path, or
/// `boundary::boundary::invalid`.
///
/// Boundaries belong under `/boundaries/`, but a valid boundary elsewhere is
/// read too. Any entry under `/boundaries/` that is not a valid boundary, a
/// boundary elsewhere that fails its schema, and a `/boundaries` that is not
/// a directory, such as a symlink, refuse every check, so a broken or
/// misplaced rule stops content instead of letting it pass unchecked.
pub(crate) fn read(host: &dyn Host) -> Result<Vec<Boundary>, PluginError> {
    let sql = format!(
        "SELECT path, COALESCE(logical, '/' || path), type, {valid}, code, \
         (SELECT body FROM records r WHERE r.source = e.source AND r.path = e.path) \
         FROM entries e WHERE source = 'worktree' AND (path GLOB 'boundaries/*' \
         OR path = 'boundaries' OR type = {kind}) ORDER BY path",
        valid = valid(),
        kind = literal(BoundaryRecord::TYPE),
    );
    let mut boundaries = Vec::new();
    let mut invalid: Vec<(String, String)> = Vec::new();
    for row in rows(host, &sql)? {
        let row = Row::deserialize(Value::Array(row)).expect(
            "the query selects two strings, a nullable string, an integer and two nullable strings",
        );
        let reason = if row.path == "boundaries" {
            Some(NOT_A_DIRECTORY.to_owned())
        } else if row.kind.as_deref() != Some(BoundaryRecord::TYPE) {
            Some(row.code.unwrap_or_else(|| NOT_A_BOUNDARY.to_owned()))
        } else if row.valid != Some(1) {
            row.code
        } else {
            let record = (row.body.as_deref())
                .and_then(|body| parse_json(body.as_bytes()).ok())
                .and_then(|record| BoundaryRecord::deserialize(&record).ok());
            match record {
                Some(record) => {
                    boundaries.push(Boundary {
                        path: row.logical,
                        record,
                    });
                    continue;
                }
                // Board admitted it against an older copy of the schema.
                None => Some("board::record::invalid".to_owned()),
            }
        };
        invalid.push((row.logical, reason.unwrap_or_default()));
    }
    if invalid.is_empty() {
        Ok(boundaries)
    } else {
        Err(refusal(&invalid))
    }
}

/// One row of [`read`]'s query.
#[derive(Deserialize)]
struct Row {
    path: String,
    logical: String,
    kind: Option<String>,
    valid: Option<u8>,
    code: Option<String>,
    body: Option<String>,
}

/// `boundary::boundary::invalid`, naming each entry and why, with the step
/// that fixes each kind.
fn refusal(invalid: &[(String, String)]) -> PluginError {
    let named: Vec<String> = (invalid.iter())
        .map(|(path, reason)| format!("{path} ({reason})"))
        .collect();
    let reasons: BTreeSet<&str> = invalid.iter().map(|(_, reason)| reason.as_str()).collect();
    let mut steps = Vec::new();
    if reasons.contains(NOT_A_BOUNDARY) {
        steps.push(MOVE_OUT.to_owned());
    }
    if reasons.contains(NOT_A_DIRECTORY) {
        steps.push(MAKE_DIRECTORY.to_owned());
    }
    let broken = (invalid.iter())
        .find(|(_, reason)| ![NOT_A_BOUNDARY, NOT_A_DIRECTORY].contains(&reason.as_str()));
    if let Some((path, _)) = broken {
        steps.push(format!(
            "run `board --personal check {path}` and fix each record that is not valid"
        ));
    }
    let boundaries: BTreeMap<&str, &str> = (invalid.iter())
        .map(|(path, reason)| (path.as_str(), reason.as_str()))
        .collect();
    PluginError {
        code: "boundary::boundary::invalid".to_owned(),
        message: format!(
            "the personal board holds boundaries that are not valid, so nothing can be checked: {}",
            named.join(", ")
        ),
        fields: Map::from_iter([(
            "boundaries".to_owned(),
            serde_json::to_value(boundaries).expect("a map of strings is JSON"),
        )]),
        hint: Some(format!("{}; then check again", steps.join("; "))),
    }
}

/// The rows of `sql`, every one of them.
pub(crate) fn rows(host: &dyn Host, sql: &str) -> Result<Vec<Vec<Value>>, PluginError> {
    (host.query(sql, Limit::ALL))
        .map(|report| report.rows)
        .map_err(|error| refused("query", &error.refusal()))
}
