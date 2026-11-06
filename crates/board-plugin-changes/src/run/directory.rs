//! Where a change's agent works: the change's worktree of the clone its
//! mount reads, or the board's root for a change on the board itself.

use std::path::PathBuf;
use std::time::Duration;

use board_ops::{Host, PluginError};
use serde_json::json;

use crate::board;
use crate::change::name_in;
use crate::diff::git;
use crate::records::BOARD_ITSELF;
use crate::refusal;

/// How long each git command that makes the worktree may take.
const GIT_DEADLINE: Duration = Duration::from_secs(30);

/// Where the agent of the change at `path` on `mount` works: the board's
/// root for a change on `/`, which has no repository, else the change's
/// worktree.
pub(super) fn directory_for(
    host: &dyn Host,
    path: &str,
    mount: &str,
) -> Result<PathBuf, PluginError> {
    if mount != BOARD_ITSELF {
        return worktree(host, path, mount);
    }
    let root = board::resolve(host, BOARD_ITSELF)?.map(|answer| answer.location.board.repository);
    root.ok_or_else(|| {
        refusal(
            "changes::run::worktree",
            format!("the board's root cannot be read to run {path} in; run with --inline true"),
            [("path", json!(path)), ("mount", json!(mount))],
        )
    })
}

/// The worktree the change's agent works in: `<clone>.worktrees/<name>` of
/// the clone the mount reads, on branch `<name>`, made from the branch the
/// mount follows when it is not there yet; the mount's own directory in
/// it. Else `changes::run::worktree`.
fn worktree(host: &dyn Host, path: &str, mount: &str) -> Result<PathBuf, PluginError> {
    let name = name_in(path);
    let failed = |why: String| {
        refusal(
            "changes::run::worktree",
            format!(
                "the worktree for {path} cannot be made: {why}; fix the clone of {mount}, or run \
                 with --inline true"
            ),
            [("path", json!(path)), ("mount", json!(mount))],
        )
    };
    let Some(answer) = board::resolve(host, mount)? else {
        return Err(failed(format!("nothing is at {mount}")));
    };
    let location = &answer.location;
    let clone = &location.board.repository;
    let follow = (location.pointers.last())
        .and_then(|pointer| pointer.follow.as_deref())
        .unwrap_or("HEAD");
    let directory = PathBuf::from(format!("{}.worktrees/{name}", clone.display()));
    if !directory.is_dir() {
        let shown = directory.display().to_string();
        let branch = format!("refs/heads/{name}");
        let held = git(
            host,
            clone,
            &["rev-parse", "--verify", "--quiet", &branch],
            GIT_DEADLINE,
        )
        .is_ok();
        let arguments: Vec<&str> = if held {
            vec!["worktree", "add", &shown, name]
        } else {
            vec!["worktree", "add", "-b", name, &shown, follow]
        };
        git(host, clone, &arguments, GIT_DEADLINE).map_err(|failure| failed(failure.why))?;
    }
    Ok(directory.join(&location.path))
}
