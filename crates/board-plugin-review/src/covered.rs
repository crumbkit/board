//! `covered`: whether reports cover every commit a pull request adds, by
//! range or patch id.
//!
//! The reports are those committed in the board's own tree, or those a
//! change attaches when the call names a change, whose commits are then its
//! mount's.

use std::collections::HashSet;
use std::path::PathBuf;

use board_model::{Code, LogicalPath, Source};
use board_ops::{Absent, Host, PluginError, literal};
use board_plugin::shape::Record;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::records::ReportRecord;
use crate::{BLOCKING, CHANGE_TYPE, PLUGIN, git, readable, refusal, refused, root, rows};

/// How many uncovered commits a refusal names.
const SHOWN: usize = 20;

/// Whether a committed report, or a report a change attaches, covers the
/// commits a pull request adds to its base.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "CoveredInput")]
pub(crate) struct Input {
    #[schemars(
        length(min = 1),
        description = "The commit the pull request merges into, in any form git resolves, such as \
                       its base commit."
    )]
    pub(crate) base: String,
    #[schemars(
        length(min = 1),
        description = "The commit to check, in any form git resolves, such as a pull request's \
                       head commit; HEAD, or with a change its head, when left out."
    )]
    pub(crate) head: Option<String>,
    #[schemars(
        extend("format" = "board-path"),
        description = "A change, by its logical path, such as /changes/fix-sync: count the reports \
                       it attaches, and read the commits from the clone its mount reads."
    )]
    pub(crate) change: Option<String>,
}

/// The commit checked, its merge base with the base, and the reports that
/// cover the commits between.
///
/// `skipped` counts the commits that only add review records and need no
/// report, `commits` the rest, and `reports` lists the reports that cover at
/// least one of them.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(rename = "CoveredOutput", deny_unknown_fields)]
pub(crate) struct Output {
    pub(crate) head: String,
    pub(crate) merge_base: String,
    pub(crate) skipped: usize,
    pub(crate) commits: usize,
    pub(crate) reports: Vec<String>,
}

/// What `covered` reads of a report of a range, which the schema gives
/// patch ids.
struct Covering {
    path: String,
    changes: bool,
    blocking: bool,
    range: String,
    patch_ids: Vec<String>,
}

/// The report at `path` holding `body`, if it is a report of a range.
fn covering(path: String, body: &str) -> Option<Covering> {
    let report: Value = serde_json::from_str(body).ok()?;
    let range = report.get("range")?.as_str()?.to_owned();
    let findings = report.get("findings").and_then(Value::as_array);
    Some(Covering {
        path,
        changes: report.get("verdict").and_then(Value::as_str) == Some("changes"),
        blocking: (findings.into_iter().flatten())
            .filter_map(|finding| finding.get("severity").and_then(Value::as_str))
            .any(|severity| BLOCKING.contains(&severity)),
        range,
        patch_ids: (report
            .get("patch_ids")
            .and_then(Value::as_array)
            .into_iter()
            .flatten())
        .filter_map(|id| id.as_str().map(str::to_owned))
        .collect(),
    })
}

/// The reports that cover every commit from the merge base of `base` and
/// the head to the head.
///
/// Commits that only add review records need no report, so committing a
/// report does not leave a head that no report covers. A report with the
/// verdict `changes` never covers. Another covers each commit its range
/// contains. It covers by patch id, as after a rebase, only when every
/// patch id it lists is a commit's in the range, so dropping a commit the
/// review saw needs a new review. A merge or an empty commit has no patch
/// id, so only a range that contains it covers it. Only reports committed
/// at HEAD in the board's own tree count, unless the call names a change:
/// then only the reports the change attaches count, the commits are those
/// of the clone its mount reads, and no commit is skipped, since the
/// reports live in the board.
pub(crate) fn covered(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let given: Input = PluginError::read_input(PLUGIN, "covered", input)?;
    let named = given.change.as_deref();
    let (repository, named_head, reports) = sources(host, &given)?;
    let head = repository.commit(&named_head)?;
    let base = repository.commit(&given.base)?;
    let merge_base = repository.merge_base(&base, &head)?;
    let added = repository.commits(&merge_base, &head)?;
    let commits = if named.is_some() {
        added.clone()
    } else {
        let mut kept = Vec::new();
        for commit in &added {
            if !repository.review_only(commit)? {
                kept.push(commit.clone());
            }
        }
        kept
    };
    let skipped = added.len() - commits.len();
    let patch_ids = repository.patch_ids(&commits)?;
    let all: HashSet<&String> = commits.iter().collect();
    let present: HashSet<&String> = patch_ids.values().collect();
    let mut uncovered: HashSet<String> = commits.iter().cloned().collect();
    let mut covering = Vec::new();
    let counted =
        (reports.iter()).filter(|report| !report.changes && (named.is_none() || !report.blocking));
    for report in counted {
        let listed: HashSet<&String> = report.patch_ids.iter().collect();
        let mut reached = repository.within(&report.range);
        if listed.is_subset(&present) {
            let by_id = (uncovered.iter())
                .filter(|commit| patch_ids.get(*commit).is_some_and(|id| listed.contains(id)));
            reached.extend(by_id.cloned().collect::<Vec<_>>());
        }
        if reached.iter().any(|commit| all.contains(commit)) {
            covering.push(report.path.clone());
        }
        uncovered.retain(|commit| !reached.contains(commit));
    }
    let output = Output {
        head,
        merge_base,
        skipped,
        commits: commits.len(),
        reports: covering,
    };
    if uncovered.is_empty() {
        return Ok(serde_json::to_value(output).expect("the output is strings and numbers"));
    }
    let missing: Vec<&String> = (commits.iter())
        .filter(|commit| uncovered.contains(*commit))
        .collect();
    Err(refusal_of(named, &output, &reports, &missing))
}

/// The repository whose commits are checked, the head it names, and the
/// reports that count: the board's own, or those the change attaches.
fn sources<'a>(
    host: &'a dyn Host,
    given: &Input,
) -> Result<(git::Repository<'a>, String, Vec<Covering>), PluginError> {
    let Some(named) = given.change.as_deref() else {
        let repository = git::at(host, &root(host)?);
        let reports = own(host, &repository)?;
        let head = given.head.clone().unwrap_or_else(|| "HEAD".to_owned());
        return Ok((repository, head, reports));
    };
    let change = change(host, named)?;
    let text = |key: &str| change.get(key).and_then(Value::as_str);
    let repository = git::at(host, &clone(host, text("mount").unwrap_or_default())?);
    let Some(head) = given
        .head
        .clone()
        .or_else(|| text("head").map(str::to_owned))
    else {
        return Err(refusal(
            "review::change::no_head",
            format!(
                "the change at {named} records no head commit, so nothing says which commits to \
                 check; record one with `board call changes.attach --path {named} --head \
                 <commit>`, or pass --head"
            ),
            [("change", json!(named))],
        ));
    };
    let refs: Vec<String> = (change.get("records").and_then(Value::as_array))
        .into_iter()
        .flatten()
        .filter_map(|record| record.as_str().map(str::to_owned))
        .collect();
    Ok((repository, head, attached(host, &refs)?))
}

/// `review::head::uncovered`, for the `missing` commits of `checked`, which
/// no report among `reports` covers.
fn refusal_of(
    named: Option<&str>,
    checked: &Output,
    reports: &[Covering],
    missing: &[&String],
) -> PluginError {
    let Output {
        head,
        merge_base,
        skipped,
        commits,
        ..
    } = checked;
    let asking = reports.iter().filter(|report| report.changes).count();
    let blocked = (reports.iter())
        .filter(|report| !report.changes && report.blocking)
        .count();
    let reviewed = format!("{merge_base}..{head}");
    let (where_, fix, blocking) = match named {
        None => (
            "in the board's own tree".to_owned(),
            format!(
                "Run `board call review.run --brief <brief>` with a brief whose range is \
                 {reviewed}, and commit it"
            ),
            String::new(),
        ),
        Some(named) => (
            format!("attached to {named}"),
            format!(
                "Attach a report whose range is {reviewed} with `board call changes.attach --path \
                 {named} --ref <report>`"
            ),
            format!(" and {blocked} have a critical or high finding"),
        ),
    };
    let skipping = if *skipped > 0 {
        format!(", not counting {skipped} commits that only add review records")
    } else {
        String::new()
    };
    refusal(
        "review::head::uncovered",
        format!(
            "{} of the {commits} commits from {merge_base} to {head} are covered by no \
             review/report/v1 {where_}{skipping}; of {} reports, {asking} ask for \
             changes{blocking}. The first is {}. {fix}",
            missing.len(),
            reports.len(),
            missing[0],
        ),
        [
            ("head", json!(head)),
            ("merge_base", json!(merge_base)),
            ("skipped", json!(skipped)),
            ("reports", json!(reports.len())),
            (
                "uncovered",
                json!(missing.iter().take(SHOWN).collect::<Vec<_>>()),
            ),
        ],
    )
}

/// Every readable report of a range in the board's own tree that HEAD
/// holds as it is on disk, by logical path. A report of a subject reviews a
/// record, not commits, so it is left out.
fn own(host: &dyn Host, repository: &git::Repository<'_>) -> Result<Vec<Covering>, PluginError> {
    let sql = format!(
        "SELECT logical, path, blob, body FROM records WHERE source = 'worktree' AND type = {} \
         AND {} ORDER BY logical",
        literal(ReportRecord::TYPE),
        readable()
    );
    let found = rows(host, &sql)?;
    let committed = repository.committed()?;
    Ok((found.iter())
        .filter_map(|row| {
            let [logical, path, blob, body] = &row[..] else {
                return None;
            };
            let held = committed.get(path.as_str()?)?;
            (Some(held.as_str()) == blob.as_str()).then_some(())?;
            covering(logical.as_str()?.to_owned(), body.as_str()?)
        })
        .collect())
}

/// The change at the logical `path`, or `review::change::not_found` when no
/// readable change is there. A change Board marks degraded for anything but
/// a broken reference counts as not there.
fn change(host: &dyn Host, path: &str) -> Result<Value, PluginError> {
    let sql = format!(
        "SELECT body FROM records WHERE logical = {} AND type = {} AND {}",
        literal(path),
        literal(CHANGE_TYPE),
        readable()
    );
    let found = (rows(host, &sql)?.into_iter().next())
        .and_then(|row| serde_json::from_str(row.first()?.as_str()?).ok());
    found.ok_or_else(|| {
        refusal(
            "review::change::not_found",
            format!(
                "no readable {CHANGE_TYPE} is at {path}; name a change `board call changes.open` \
                 lists, or run `board check {path}` to see why Board cannot read it"
            ),
            [("change", json!(path))],
        )
    })
}

/// The clone the mount at `mount` reads its pinned commit from, or
/// `review::mount::not_found`. Only a mount's root names a mount: a path in
/// the board's own tree, or below a mount's root, does not.
fn clone(host: &dyn Host, mount: &str) -> Result<PathBuf, PluginError> {
    let not_found = || {
        refusal(
            "review::mount::not_found",
            format!(
                "no mount is at {mount}, so its commits cannot be read; name the repository by the \
                 logical path of its mount, such as /repos/<name>, and see what is mounted with \
                 `board ls /repos`"
            ),
            [("mount", json!(mount))],
        )
    };
    let Ok(logical) = mount.parse::<LogicalPath>() else {
        return Err(not_found());
    };
    let absent = |code: &str| code == Code::PathNotFound.as_str();
    let answers = match host.refs(&[logical], Absent::Refuse, &[]) {
        Ok(answers) => answers,
        Err(error) if absent(error.refusal().code.as_str()) => return Err(not_found()),
        Err(error) => return Err(refused("refs", &error.refusal())),
    };
    let location = match answers.into_iter().next() {
        Some(Ok(report)) => report.location,
        Some(Err(error)) if absent(error.refusal().code.as_str()) => return Err(not_found()),
        Some(Err(error)) => return Err(refused("refs", &error.refusal())),
        None => return Err(not_found()),
    };
    match location.board.source {
        Source::Commit(_) if location.path.is_empty() => Ok(location.board.repository),
        _ => Err(not_found()),
    }
}

/// Every readable report of a range among `refs`, each once, in the order
/// the change first attaches them.
///
/// The changes plugin's review gate counts the reports a change attaches as
/// Board reads them, committed or not, so this does too.
fn attached(host: &dyn Host, refs: &[String]) -> Result<Vec<Covering>, PluginError> {
    if refs.is_empty() {
        return Ok(Vec::new());
    }
    let listed: Vec<String> = refs.iter().map(|path| literal(path)).collect();
    let sql = format!(
        "SELECT logical, body FROM records WHERE logical IN ({}) AND type = {} AND {}",
        listed.join(", "),
        literal(ReportRecord::TYPE),
        readable()
    );
    let found = rows(host, &sql)?;
    let body = |path: &str| {
        (found.iter()).find_map(|row| {
            (row.first()?.as_str()? == path).then_some(())?;
            row.get(1)?.as_str()
        })
    };
    let mut seen = HashSet::new();
    Ok((refs.iter())
        .filter(|path| seen.insert(path.as_str()))
        .filter_map(|path| covering(path.clone(), body(path)?))
        .collect())
}
