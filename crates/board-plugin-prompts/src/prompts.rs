//! What `get` and `list` both read: the marked repos, the root of each by
//! the commit it pins, and the files a condition selects, in one query.
//! Neither capability syncs a repo.

use std::collections::{BTreeMap, BTreeSet};

use board_ops::{Host, Limit, PluginError, literal};
use board_plugin::shape::TREE_CODES;
use serde_json::{Value, json};

use crate::records::{PromptRecord, ReposRecord};
use crate::refusal;
use crate::repos::{self, Listed, json_body, shown};
use board_plugin::shape::Record;

/// The list of marked repos, in the board's own working tree.
pub(crate) const MARKS: &str = "prompts/repos.json";

/// What a query reads of each file it selects: the commit it was read
/// from, the root of the path Board indexed it at, its logical path, its
/// path in the repo, its type, Board code and frontmatter.
const COLUMNS: &str = "e.source, substr(e.logical, 1, length(e.logical) - length(e.path)), \
                       e.logical, e.path, e.type, e.code, r.body";

/// One file a query selected, by [`COLUMNS`].
#[derive(Clone, Debug)]
pub(crate) struct File {
    pub(crate) source: String,
    pub(crate) at: String,
    pub(crate) logical: String,
    pub(crate) kind: Value,
    pub(crate) code: Value,
    pub(crate) frontmatter: Value,
}

impl File {
    /// Whether Board marks the file degraded for a code other than a
    /// broken link.
    pub(crate) fn degraded(&self) -> bool {
        (self.code.as_str()).is_some_and(|code| !TREE_CODES.contains(&code))
    }
}

/// What [`query`] reads.
pub(crate) struct Read {
    /// The marked repos.
    pub(crate) found: Vec<Listed>,
    /// The root of each source: `worktree` is `/`.
    pub(crate) roots: BTreeMap<String, String>,
    pub(crate) files: Vec<File>,
}

/// The files `list` reads: each Markdown file under a `prompts/` directory
/// that is a `prompts/prompt/v1` record, or that Board marks degraded with
/// a code other than a tree code. Plain Markdown, such as an eval's
/// evidence, has no type and no such code, so it is left out.
pub(crate) fn listed_files() -> String {
    let codes: Vec<String> = TREE_CODES.iter().map(|code| literal(code)).collect();
    format!(
        "(e.type = {} OR (e.code IS NOT NULL AND e.code NOT IN ({}))) \
         AND e.path GLOB '*.md' AND ('/' || e.path) GLOB '*/prompts/*'",
        literal(PromptRecord::TYPE),
        codes.join(", ")
    )
}

/// The mounts, the marks and the files `condition` selects, in one query.
/// Each mount's pointer is in the board's own working tree, never inside
/// another mount.
fn sql(condition: &str) -> String {
    let record = literal(repos::RECORD);
    let marks = literal(MARKS);
    format!(
        "SELECT 'mount', '/' || substr(m.path, 1, length(m.path) - 5), m.target, s.repository, \
                e.type, e.status, e.code, r.body
         FROM mounts AS m
         LEFT JOIN sources AS s ON s.source = m.target
         LEFT JOIN entries AS e ON e.source = m.target AND e.path = {record}
         LEFT JOIN records AS r ON r.source = m.target AND r.path = {record}
         WHERE m.source = 'worktree' AND m.code IS NULL AND m.target IS NOT NULL
         UNION ALL
         SELECT 'marks', e.type, e.status, e.code, r.body, NULL, NULL, NULL
         FROM entries AS e LEFT JOIN records AS r ON r.logical = e.logical
         WHERE e.source = 'worktree' AND e.path = {marks}
         UNION ALL
         SELECT * FROM (
           SELECT 'file', {COLUMNS} FROM entries AS e
           LEFT JOIN records AS r ON r.logical = e.logical
           WHERE {condition} ORDER BY e.logical
         )"
    )
}

/// `prompts::board::refused`, for Board's refusal of `board <command>`.
pub(crate) fn refused(command: &str, refusal: &board_model::Refusal) -> PluginError {
    PluginError::board_refused("prompts", command, refusal)
}

/// The marked repos, the root of each by the commit it pins, and the files
/// `condition` selects. A repo is marked when `/prompts/repos.json` in the
/// board's own tree names it and its mount pointer is in that same tree,
/// so nothing a mounted repository commits can mark it.
pub(crate) fn query(host: &dyn Host, condition: &str) -> Result<Read, PluginError> {
    let report = (host.query(&sql(condition), Limit::ALL))
        .map_err(|error| refused("query", &error.refusal()))?;
    let of = |kind: &str| -> Vec<&[Value]> {
        (report.rows.iter())
            .filter(|row| row.first().and_then(Value::as_str) == Some(kind))
            .map(|row| &row[1..])
            .collect()
    };
    let marked = marks(&of("marks"))?;
    let mounts: Vec<&[Value]> = (of("mount").into_iter())
        .filter(|row| marked.contains(&repos::root(&shown(&row[0]))))
        .collect();
    let found = repos::parsed("prompts", &mounts)?;
    let mut roots = BTreeMap::from([("worktree".to_owned(), "/".to_owned())]);
    for listed in &found {
        if let Some(source) = &listed.source {
            roots.insert(source.clone(), listed.root.clone());
        }
    }
    let files = (of("file").into_iter())
        .map(|row| File {
            source: shown(&row[0]),
            at: shown(&row[1]),
            logical: shown(&row[2]),
            kind: row[4].clone(),
            code: row[5].clone(),
            frontmatter: row[6].clone(),
        })
        .collect();
    Ok(Read {
        found,
        roots,
        files,
    })
}

/// The roots `/prompts/repos.json` names; none without the file, and
/// refused when it is not valid.
fn marks(rows: &[&[Value]]) -> Result<BTreeSet<String>, PluginError> {
    let Some(row) = rows.first() else {
        return Ok(BTreeSet::new());
    };
    let (kind, status, code, body) = (&row[0], &row[1], &row[2], &row[3]);
    if kind.as_str() != Some(ReposRecord::TYPE) || status.as_str() != Some("ok") {
        let why = format!(
            "/{MARKS} is not a valid {} record (type {}, status {}, code {}), so no mount's \
             prompts are admitted; run `board check /{MARKS}` to see what is wrong",
            ReposRecord::TYPE,
            shown(kind),
            shown(status),
            shown(code)
        );
        let fields = [("path", json!(format!("/{MARKS}"))), ("code", code.clone())];
        return Err(refusal("prompts::repos::invalid", why, fields));
    }
    let body = json_body(body);
    let mounts = body.get("mounts").and_then(Value::as_array);
    Ok(mounts
        .into_iter()
        .flatten()
        .map(|at| repos::root(&shown(at)))
        .collect())
}
