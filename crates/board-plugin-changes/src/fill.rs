//! Values Board finds for a change that lacks them, so an agent need not
//! read a remedy and attach them by hand.
//!
//! `next` and `advance` record the route CODEOWNERS gives a change that has
//! none, as `changes start` derives it, and the HEAD of the caller's
//! checkout as the head of a change that has none. The checkout must be of
//! the change's mount, on a branch other than the one the mount follows,
//! and hold a commit that branch lacks. When that commit may be another
//! change's work, or touches none of the plan's paths, they only suggest
//! it. `attach` stores the full commit a short head names in the mount's
//! clone. A change with no task is not filled: the task remedy names the
//! one open task whose title is the change's.

use std::path::{Path, PathBuf};
use std::time::Duration;

use board_ops::{Host, PluginError, literal};
use serde_json::json;

use crate::board::{rows, text};
use crate::change::{change_types, plan_at, read, touch, write};
use crate::diff::{Mounted, exited, git, mounted, outside};
use crate::flow::{Field, Kind};
use crate::gates::{Check, Observed, Status};
use crate::records::{ChangeRecord, PAUSED, Route, TASK_TYPES, is_final};
use crate::refusal;
use crate::repo::{CODEOWNERS, ask_first, files, owners, route};
use crate::wip::followed;

/// How long each git command here may take.
const DEADLINE: Duration = Duration::from_secs(10);
/// The hex digits of a full commit id.
const FULL: usize = 40;

/// What the caller's checkout gives a change with no head: its HEAD to
/// record, a HEAD only to suggest and why, or nothing.
enum Found {
    Fill { top: PathBuf, head: String },
    Suggest { head: String, why: String },
    Nothing,
}

/// Records the route and head the change at `path` lacks, where Board can
/// find them, and says what it recorded or only suggests, one line for
/// each.
///
/// The write expects the change as it was read, so a change written since
/// is `board::put::stale` and nothing is written.
pub(crate) fn missing(host: &dyn Host, path: &str) -> Result<Vec<String>, PluginError> {
    let (mut change, blob) = read(host, path)?;
    if is_final(&change.phase) || change.phase == PAUSED {
        return Ok(Vec::new());
    }
    let mut said = Vec::new();
    let mut wrote = false;
    if change.route.is_none() {
        let routed = derived_route(host, &change.mount)?;
        change.route = Some(routed);
        wrote = true;
        let shown = json!(routed);
        said.push(format!(
            "route filled as {}; set another with `board changes attach --path {path} --route \
             <route>`",
            shown.as_str().unwrap_or_default()
        ));
    }
    if change.head.is_none() {
        match checkout(host, path, &change)? {
            Found::Fill { top, head } => {
                said.push(format!(
                    "head filled from {} at {head}; set another with `board changes attach \
                     --path {path} --head <sha>`",
                    top.display()
                ));
                change.head = Some(head);
                wrote = true;
            }
            Found::Suggest { head, why } => said.push(format!(
                "head not filled: {why}; if {head} is this change's work, record it with `board \
                 changes attach --path {path} --head {head}`"
            )),
            Found::Nothing => {}
        }
    }
    if wrote {
        write(host, path, &blob, &touch(host, change))?;
    }
    Ok(said)
}

/// The route the CODEOWNERS of the repository `mount` reads gives a change
/// whose owner is not named, as `changes start` derives it without a
/// handle: `unowned` when it names no owners.
fn derived_route(host: &dyn Host, mount: &str) -> Result<Route, PluginError> {
    if mount.trim_end_matches('/').is_empty() {
        return Ok(Route::Unowned);
    }
    let found = files(host, mount.trim_end_matches('/'), &CODEOWNERS)?;
    let rules = owners(found.first().map_or("", |(_, text)| text.as_str()));
    Ok(route(&rules, &ask_first(&rules, None)))
}

/// What the caller's checkout gives the change at `path`: its HEAD to
/// record when the checkout is of the repository the change's mount reads,
/// has a branch checked out other than the one the mount follows, holds a
/// commit that branch lacks, and that commit is tied to the change.
fn checkout(host: &dyn Host, path: &str, change: &ChangeRecord) -> Result<Found, PluginError> {
    let Some(caller) = host.settings().caller.clone() else {
        return Ok(Found::Nothing);
    };
    let Ok(at) = mounted(host, &change.mount)? else {
        return Ok(Found::Nothing);
    };
    let run = |directory: &Path, arguments: &[&str]| git(host, directory, arguments, DEADLINE).ok();
    let Some(shown) = run(&caller, &["rev-parse", "--show-toplevel", "HEAD"]) else {
        return Ok(Found::Nothing);
    };
    let mut lines = shown.lines().map(str::trim);
    let (Some(top), Some(head)) = (lines.next(), lines.next()) else {
        return Ok(Found::Nothing);
    };
    let branch = run(&caller, &["rev-parse", "--symbolic-full-name", "HEAD"]);
    let Some(branch) = branch
        .as_deref()
        .and_then(|name| name.trim().strip_prefix("refs/heads/"))
    else {
        return Ok(Found::Nothing);
    };
    if default_branch(host, &at).is_none_or(|default| default == branch) {
        return Ok(Found::Nothing);
    }
    let top = PathBuf::from(top);
    if top != at.repository && followed(host, &top).key != followed(host, &at.repository).key {
        return Ok(Found::Nothing);
    }
    if !beyond_followed(host, &at, &top, head) {
        return Ok(Found::Nothing);
    }
    let head = head.to_owned();
    if let Some(why) = another_changes(host, path, change, (&top, &at.follow), &head)? {
        return Ok(Found::Suggest { head, why });
    }
    if let Some(why) = off_plan(host, change, &at, &top, &head)? {
        return Ok(Found::Suggest { head, why });
    }
    Ok(Found::Fill { top, head })
}

/// Whether the branch the mount follows lacks `head`. git exits 1 when it
/// does. It exits 128 when the mount's clone lacks `head`, as for a commit
/// not yet pushed from a separate clone, and then the checkout at `top`
/// judges against its own ref of that name, which may be newer than the
/// clone's. Any other answer, or 128 in the checkout too, as when it has
/// no ref of that name, is not proof.
fn beyond_followed(host: &dyn Host, at: &Mounted, top: &Path, head: &str) -> bool {
    let arguments = [
        "merge-base",
        "--is-ancestor",
        "--end-of-options",
        head,
        &at.follow,
    ];
    let code = |directory: &Path| {
        exited(host, directory, &arguments, DEADLINE, &[0, 1, 128])
            .ok()
            .map(|(code, _)| code)
    };
    match code(&at.repository) {
        Some(1) => true,
        Some(128) => code(top) == Some(1),
        _ => false,
    }
}

/// Why `head` may be another open change's work: another open change on
/// the same mount records it, or records a commit beyond `follow`, the
/// branch the mount follows, that `head` builds on or that builds on
/// `head`; else none. The checkout at `top` judges the commits.
fn another_changes(
    host: &dyn Host,
    path: &str,
    change: &ChangeRecord,
    (top, follow): (&Path, &str),
    head: &str,
) -> Result<Option<String>, PluginError> {
    let sql = format!(
        "SELECT logical, json_extract(body, '$.head') FROM records WHERE type IN ({}) AND code IS \
         NULL AND json_extract(body, '$.mount') = {} AND logical <> {} AND json_extract(body, \
         '$.head') IS NOT NULL AND coalesce(json_extract(body, '$.phase'), '') NOT IN ('done', \
         'dropped') ORDER BY logical",
        change_types(),
        literal(&change.mount),
        literal(path)
    );
    let ancestor = |older: &str, newer: &str| {
        let arguments = [
            "merge-base",
            "--is-ancestor",
            "--end-of-options",
            older,
            newer,
        ];
        exited(host, top, &arguments, DEADLINE, &[0, 1, 128])
            .ok()
            .map(|(code, _)| code)
            == Some(0)
    };
    for row in rows(host, &sql)? {
        let (Some(other), Some(theirs)) = (text(row.first()), text(row.get(1))) else {
            continue;
        };
        if head.starts_with(theirs) || theirs.starts_with(head) {
            return Ok(Some(format!("{other} records {theirs}")));
        }
        let built_on = ancestor(theirs, head) && !ancestor(theirs, follow);
        if built_on || ancestor(head, theirs) {
            return Ok(Some(format!(
                "{other} records {theirs}, on the same line of work as {head}"
            )));
        }
    }
    Ok(None)
}

/// Why `head` may not be the change's work: the change's plan has paths,
/// and the commits from where `head` left the branch the mount follows
/// touch none of them, or git cannot list those commits' files in the
/// checkout at `top`; else none.
fn off_plan(
    host: &dyn Host,
    change: &ChangeRecord,
    at: &Mounted,
    top: &Path,
    head: &str,
) -> Result<Option<String>, PluginError> {
    let planned = match &change.plan {
        Some(plan) => plan_at(host, plan)?.and_then(|(record, _)| record.paths),
        None => None,
    };
    let Some(planned) = planned.filter(|paths| !paths.is_empty()) else {
        return Ok(None);
    };
    // The checkout may have no ref of the followed branch's name, as a
    // fresh clone has none for `main`, so the diff starts from the commit
    // that branch names in the mount's clone.
    let follow_commit = format!("{}^{{commit}}", at.follow);
    let verify = ["rev-parse", "--verify", "--end-of-options", &follow_commit];
    let base = match git(host, &at.repository, &verify, DEADLINE) {
        Ok(base) => base.trim().to_owned(),
        Err(failed) => return Ok(Some(failed.why)),
    };
    let range = format!("{base}...{head}");
    let arguments = [
        "diff",
        "--name-only",
        "-z",
        "--no-renames",
        "--end-of-options",
        &range,
    ];
    let names = match git(host, top, &arguments, DEADLINE) {
        Ok(names) => names,
        Err(failed) => return Ok(Some(failed.why)),
    };
    let files = at.logical(&names);
    if outside(host, &files, &planned)?.len() < files.len() {
        return Ok(None);
    }
    Ok(Some(format!(
        "its commits since {} touch none of the plan's paths",
        at.follow
    )))
}

/// The name of the branch the mount follows, such as `main` for a mount
/// that follows `origin/main`, or none when git cannot name it.
fn default_branch(host: &dyn Host, at: &Mounted) -> Option<String> {
    let arguments = [
        "rev-parse",
        "--symbolic-full-name",
        "--end-of-options",
        &at.follow,
    ];
    branch_named(&git(host, &at.repository, &arguments, DEADLINE).ok()?)
}

/// The branch `git rev-parse --symbolic-full-name` names in `shown`: `main`
/// for `refs/heads/main` or `refs/remotes/origin/main`, or none for a name
/// that is no branch, such as a tag or a commit.
fn branch_named(shown: &str) -> Option<String> {
    // git echoes `--end-of-options` on a line before the name.
    let name = shown.lines().map(str::trim).next_back()?;
    if let Some(branch) = name.strip_prefix("refs/heads/") {
        return Some(branch.to_owned());
    }
    let (_, branch) = name.strip_prefix("refs/remotes/")?.split_once('/')?;
    Some(branch.to_owned())
}

/// The full commit `head` names in the clone `mount` reads, for the change
/// at `path`: `head` itself when it is already full, else the one commit
/// whose id starts with it. No commit is `changes::attach::head_unknown`,
/// and several are `changes::attach::head_ambiguous` with the matches.
pub(crate) fn full_head(
    host: &dyn Host,
    path: &str,
    mount: &str,
    head: String,
) -> Result<String, PluginError> {
    if head.len() >= FULL {
        return Ok(head);
    }
    let unknown = |why: &str| {
        refusal(
            "changes::attach::head_unknown",
            format!(
                "{head} names no commit Board can find for the change at {path}: {why}; give \
                 the full 40-digit commit, or fetch it into the mounted repository"
            ),
            [("path", json!(path)), ("head", json!(head))],
        )
    };
    let at = mounted(host, mount)?.map_err(|why| unknown(&why))?;
    let repository = at.repository.as_path();
    let asked = format!("--disambiguate={head}");
    let listed = git(host, repository, &["rev-parse", &asked], DEADLINE)
        .map_err(|failed| unknown(&failed.why))?;
    let mut matches = Vec::new();
    for object in listed
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let kind = git(host, repository, &["cat-file", "-t", object], DEADLINE)
            .map_err(|failed| unknown(&failed.why))?;
        if kind.trim() == "commit" {
            matches.push(object.to_owned());
        }
    }
    matches.sort();
    match matches.as_slice() {
        [one] => Ok(one.clone()),
        [] => Err(unknown(&format!(
            "no commit in {} starts with it",
            repository.display()
        ))),
        _ => Err(refusal(
            "changes::attach::head_ambiguous",
            format!(
                "{head} starts {} commits in {}: {}; give more digits of the one you mean",
                matches.len(),
                repository.display(),
                matches.join(", ")
            ),
            [
                ("path", json!(path)),
                ("head", json!(head)),
                ("matches", json!(matches)),
            ],
        )),
    }
}

/// Names the task in the remedy of each missing `task` check of `gate`,
/// when the change has none and exactly one open task's title is the
/// change's title.
pub(crate) fn name_task(
    host: &dyn Host,
    observed: &Observed,
    gate: &mut [Check],
) -> Result<(), PluginError> {
    let change = &observed.change;
    let ids: Vec<&str> = (observed.checks().iter())
        .filter(|each| each.kind.known() == Some(&Kind::Field) && each.field == Some(Field::Task))
        .map(|each| each.id.as_str())
        .collect();
    let missing =
        |each: &Check| each.status == Status::Missing && ids.contains(&each.check.as_str());
    if change.task.is_some() || !gate.iter().any(missing) {
        return Ok(());
    }
    let types: Vec<String> = TASK_TYPES.iter().map(|each| literal(each)).collect();
    let sql = format!(
        "SELECT logical FROM records WHERE type IN ({}) AND code IS NULL AND \
         json_extract(body, '$.title') = {} AND coalesce(json_extract(body, '$.status'), \
         'open') NOT IN ('done', 'dropped') ORDER BY logical LIMIT 2",
        types.join(", "),
        literal(&change.title)
    );
    let found = rows(host, &sql)?;
    let [only] = found.as_slice() else {
        return Ok(());
    };
    let Some(task) = text(only.first()) else {
        return Ok(());
    };
    for each in gate.iter_mut().filter(|each| missing(each)) {
        each.remedy = Some(format!(
            "Set the change's task with `board changes attach --path {} --task {task}`.",
            observed.path
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
