//! Values Board finds for a change that lacks them, so an agent need not
//! read a remedy and attach them by hand.
//!
//! `next` and `advance` record the HEAD of the caller's checkout as the
//! head of a change that has none, when that checkout is of the change's
//! mount and on a branch other than the one the mount follows, and record
//! `unowned` as the route of a change that has none. `attach` stores the
//! full commit a short head names in the mount's clone. A change with no
//! task is not filled: the task remedy names the one open task whose title
//! is the change's.

use std::path::{Path, PathBuf};
use std::time::Duration;

use board_ops::{Host, PluginError, literal};
use serde_json::json;

use crate::board::{rows, text};
use crate::change::{read, touch, write};
use crate::diff::{Mounted, git, mounted};
use crate::flow::{Field, Kind};
use crate::gates::{Check, Observed, Status};
use crate::records::{PAUSED, Route, TASK_TYPES, is_final};
use crate::refusal;
use crate::wip::followed;

/// How long each git command here may take.
const DEADLINE: Duration = Duration::from_secs(10);
/// The hex digits of a full commit id.
const FULL: usize = 40;

/// Records the route and head the change at `path` lacks, where Board can
/// find them, and says what it recorded, one line for each.
///
/// The write expects the change as it was read, so a change written since
/// is `board::put::stale` and nothing is written.
pub(crate) fn missing(host: &dyn Host, path: &str) -> Result<Vec<String>, PluginError> {
    let (mut change, blob) = read(host, path)?;
    if is_final(&change.phase) || change.phase == PAUSED {
        return Ok(Vec::new());
    }
    let mut said = Vec::new();
    if change.route.is_none() {
        change.route = Some(Route::Unowned);
        said.push(format!(
            "route filled as unowned; set another with `board changes attach --path {path} \
             --route <route>`"
        ));
    }
    if change.head.is_none()
        && let Some((checkout, head)) = checkout(host, &change.mount)?
    {
        said.push(format!(
            "head filled from {} at {head}; set another with `board changes attach --path \
             {path} --head <sha>`",
            checkout.display()
        ));
        change.head = Some(head);
    }
    if !said.is_empty() {
        write(host, path, &blob, &touch(host, change))?;
    }
    Ok(said)
}

/// The top of the caller's checkout and its HEAD, when the checkout is of
/// the repository `mount` reads and has a branch checked out other than
/// the one the mount follows, whose HEAD that branch does not already hold,
/// as on a branch with no commit of its own yet; else none.
fn checkout(host: &dyn Host, mount: &str) -> Result<Option<(PathBuf, String)>, PluginError> {
    let Some(caller) = host.settings().caller.clone() else {
        return Ok(None);
    };
    let Ok(at) = mounted(host, mount)? else {
        return Ok(None);
    };
    let run = |directory: &Path, arguments: &[&str]| git(host, directory, arguments, DEADLINE).ok();
    let Some(shown) = run(&caller, &["rev-parse", "--show-toplevel", "HEAD"]) else {
        return Ok(None);
    };
    let mut lines = shown.lines().map(str::trim);
    let (Some(top), Some(head)) = (lines.next(), lines.next()) else {
        return Ok(None);
    };
    let branch = run(&caller, &["rev-parse", "--symbolic-full-name", "HEAD"]);
    let Some(branch) = branch
        .as_deref()
        .and_then(|name| name.trim().strip_prefix("refs/heads/"))
    else {
        return Ok(None);
    };
    if default_branch(host, &at).is_none_or(|default| default == branch) {
        return Ok(None);
    }
    let top = PathBuf::from(top);
    if top != at.repository && followed(host, &top).key != followed(host, &at.repository).key {
        return Ok(None);
    }
    // git exits 1 when the followed branch lacks HEAD, and 128 when the
    // mount's clone lacks it, as before a fetch; both may be filled.
    let on_followed = [
        "merge-base",
        "--is-ancestor",
        "--end-of-options",
        head,
        &at.follow,
    ];
    if run(&at.repository, &on_followed).is_some() {
        return Ok(None);
    }
    Ok(Some((top, head.to_owned())))
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
