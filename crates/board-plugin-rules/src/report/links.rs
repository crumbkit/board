//! What a report's rows link to outside the rule: the boundaries that apply
//! a rule to text such as a pull request body, the suite that judges a
//! rule, and the record that left an obligation owed.

use std::collections::{BTreeMap, BTreeSet};

use board_model::{LogicalPath, parse_json};
use board_ops::{Host, PluginError, literal};
use serde::Deserialize;
use serde_json::Value;

use super::place::Place;
use super::{Refused, Row, Severity, Source};
use crate::glob::glob;
use crate::index::{self, readable};
use crate::records::{OWED_BY_TYPES, SUITE_TYPE};

/// The type of a boundary, which the boundary plugin ships.
const BOUNDARY_TYPE: &str = "boundary/boundary/v1";

/// The type of a pull request, whose number and page a row gives.
const PULL_TYPE: &str = "github/pull/v1";

/// Each boundary in the board's own tree, whether Board reads it as valid,
/// and its body.
fn boundaries_sql() -> String {
    format!(
        "SELECT logical, {readable} AS valid, COALESCE(code, ''), body FROM records \
         WHERE type = {kind} AND source = 'worktree' AND logical IS NOT NULL ORDER BY logical",
        readable = readable!(),
        kind = literal(BOUNDARY_TYPE),
    )
}

/// The fields of a boundary a report reads: where it sends to, the
/// checkouts it is limited to, and the rules it checks.
#[derive(Deserialize)]
struct BoundaryRead {
    to: String,
    from: Option<Vec<String>>,
    checks: Vec<CheckRead>,
}

/// A rule a boundary checks, named `control` before the rename.
#[derive(Deserialize)]
struct CheckRead {
    #[serde(alias = "control")]
    rule: String,
}

impl BoundaryRead {
    /// Whether the boundary reaches the repository named `name` at
    /// `mount`: its `from` holds the mount or a path above it, or it has no
    /// `from` and its `to` matches `github:<name>`, without case.
    fn reaches(&self, mount: &LogicalPath, name: &str) -> bool {
        match &self.from {
            Some(paths) => paths.iter().any(|path| {
                path.parse::<LogicalPath>()
                    .is_ok_and(|path| crate::check::under(mount, &path))
            }),
            None => glob(
                &self.to.to_lowercase(),
                &format!("github:{name}").to_lowercase(),
            ),
        }
    }
}

/// The rules that a boundary reaching the repository named `name` checks,
/// by path. A boundary Board does not read as valid is a
/// `rules::report::boundary_unreadable` refusal, and its rules gain no
/// `body` target from it.
pub(crate) fn bodies(
    host: &dyn Host,
    place: &Place,
    name: &str,
    refused: &mut Vec<Refused>,
) -> Result<BTreeSet<String>, PluginError> {
    let mount = place
        .mount
        .as_ref()
        .map_or_else(LogicalPath::root, |(mount, _)| mount.clone());
    let mut named = BTreeSet::new();
    for row in index::rows(host, &boundaries_sql())?.rows {
        let [Value::String(logical), valid, code, body] = &row[..] else {
            continue;
        };
        let read = (valid.as_i64() == Some(1))
            .then(|| body.as_str())
            .flatten()
            .and_then(|body| parse_json(body.as_bytes()).ok())
            .and_then(|record| BoundaryRead::deserialize(&record).ok());
        let Some(boundary) = read else {
            let code = code.as_str().filter(|code| !code.is_empty());
            refused.push(Refused {
                code: "rules::report::boundary_unreadable".to_owned(),
                severity: Severity::Refusal,
                message: format!(
                    "Board marks the boundary {logical} invalid with {}, so the rules it checks \
                     gain no body target from it; run `board check {logical}` for why",
                    code.unwrap_or("board::record::invalid")
                ),
                rules: vec![logical.clone()],
            });
            continue;
        };
        if boundary.reaches(&mount, name) {
            named.extend(boundary.checks.into_iter().map(|check| check.rule));
        }
    }
    Ok(named)
}

/// A record a row links to, as the index holds it.
struct Linked {
    kind: String,
    valid: bool,
    body: Option<String>,
}

/// The fields of a pull request a row gives.
#[derive(Deserialize)]
struct PullRead {
    number: u64,
    url: String,
}

/// Fills each row's `judged_by` and `owed_by` from the records they name,
/// read in one query. A `judged_by` that names no valid suite is a
/// `rules::report::suite_unreadable` warning, and an `owed_by` that names
/// no valid record of a type it may name, or a pull request without its
/// number and page, is a `rules::report::owed_by_unreadable` warning; each
/// row keeps the path. A repository's own rule names paths in the
/// repository, so they are looked up below its mount.
pub(crate) fn linked(
    host: &dyn Host,
    place: &Place,
    rows: &mut [Row],
    refused: &mut Vec<Refused>,
) -> Result<(), PluginError> {
    let lookup = |row: &Row, path: &str| match (&row.source, &place.mount) {
        (Source::Repository, Some((mount, _))) => {
            format!("{}{path}", mount.as_str().trim_end_matches('/'))
        }
        _ => path.to_owned(),
    };
    let mut wanted = BTreeSet::new();
    for row in rows.iter() {
        let owed = row.owed_by.as_ref().map(|owed| owed.path.as_str());
        for path in row.judged_by.as_deref().into_iter().chain(owed) {
            wanted.insert(lookup(row, path));
        }
    }
    let found = records_at(host, &wanted)?;
    for row in rows.iter_mut() {
        if let Some(suite) = &row.judged_by {
            let at = found.get(&lookup(row, suite));
            if !at.is_some_and(|linked| linked.valid && linked.kind == SUITE_TYPE) {
                refused.push(Refused {
                    code: "rules::report::suite_unreadable".to_owned(),
                    severity: Severity::Warning,
                    message: format!(
                        "the rule {}'s judged_by names {suite}, which Board does not read as a \
                         valid suite",
                        row.id
                    ),
                    rules: vec![row.id.clone()],
                });
            }
        }
        let Some(owed) = &row.owed_by else {
            continue;
        };
        let at = found.get(&lookup(row, &owed.path));
        let kind = at.map(|linked| linked.kind.clone());
        let usable = at.filter(|linked| linked.valid && OWED_BY_TYPES.contains(&&*linked.kind));
        let pull = usable
            .filter(|linked| linked.kind == PULL_TYPE)
            .and_then(|linked| linked.body.as_deref())
            .and_then(|body| parse_json(body.as_bytes()).ok())
            .and_then(|record| PullRead::deserialize(&record).ok());
        let unread = usable.is_none() || (kind.as_deref() == Some(PULL_TYPE) && pull.is_none());
        if unread {
            refused.push(Refused {
                code: "rules::report::owed_by_unreadable".to_owned(),
                severity: Severity::Warning,
                message: format!(
                    "the obligation {}'s owed_by names {}, which Board does not read as a valid \
                     pull request, review report or decision; `board github ref` writes a pull \
                     request's record",
                    row.id, owed.path
                ),
                rules: vec![row.id.clone()],
            });
        }
        let owed = row
            .owed_by
            .as_mut()
            .expect("the row names what left it owed");
        owed.kind = kind;
        if let Some(pull) = pull {
            owed.number = Some(pull.number);
            owed.url = Some(pull.url);
        }
    }
    Ok(())
}

/// Each record at one of `paths`, by its logical path.
fn records_at(
    host: &dyn Host,
    paths: &BTreeSet<String>,
) -> Result<BTreeMap<String, Linked>, PluginError> {
    if paths.is_empty() {
        return Ok(BTreeMap::new());
    }
    let listed: Vec<String> = paths.iter().map(|path| literal(path)).collect();
    let sql = format!(
        "SELECT logical, type, {readable}, body FROM records WHERE logical IN ({})",
        listed.join(", "),
        readable = readable!(),
    );
    let mut found = BTreeMap::new();
    for row in index::rows(host, &sql)?.rows {
        let [Value::String(logical), Value::String(kind), valid, body] = &row[..] else {
            continue;
        };
        found.insert(
            logical.clone(),
            Linked {
                kind: kind.clone(),
                valid: valid.as_i64() == Some(1),
                body: body.as_str().map(str::to_owned),
            },
        );
    }
    Ok(found)
}
