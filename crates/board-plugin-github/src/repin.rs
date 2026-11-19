//! After a merge, moves the merged repository's mount to the commit the
//! branch it follows is at on its remote, as `git fetch` and `board mount
//! update` do. The pull request has already merged, so a step that fails
//! here leaves the mount at its pin and gives the warning
//! `github::merge::mount`, with the command that moves it by hand.

use std::path::Path;

use board_model::{LogicalPath, PluginCode, PluginName, Refusal};
use board_ops::{Absent, Host, LOCK_WAIT, PluginError};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{Map, json};

use crate::git;
use crate::merge::SAME_BOARD;

/// The code of every warning this module gives.
const CODE: &str = "github::merge::mount";

/// The mount a merge moved, and the commits it was and is pinned at.
/// `wrote` is false when the mount was already at the new commit.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "MergeMount")]
pub(crate) struct Moved {
    path: String,
    old: String,
    new: String,
    wrote: bool,
}

/// Why a merge left the mount at its pin. `cause` is the code of the step
/// that failed, when one did, and `command` moves the mount by hand.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "MergeMountWarning")]
pub(crate) struct Warning {
    code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    cause: Option<String>,
    message: String,
    command: String,
}

/// After a merge, moves the mount at `path` in `board`, the personal board
/// when `personal`. A merge GitHub only `queued` leaves it. A mount left at
/// its pin is warned of through `host`.
pub(crate) fn repin(
    host: &dyn Host,
    board: &dyn Host,
    path: Option<&str>,
    personal: bool,
    queued: bool,
) -> (Option<Moved>, Option<Warning>) {
    match moved(host, board, path, personal, queued) {
        Ok(moved) => (Some(moved), None),
        Err(warning) => {
            host.warn(&warning.refusal(personal));
            (None, Some(warning))
        }
    }
}

/// [`repin`]'s steps: resolve the mount, find the remote branch it
/// follows, fetch it, and update the mount.
fn moved(
    host: &dyn Host,
    board: &dyn Host,
    path: Option<&str>,
    personal: bool,
    queued: bool,
) -> Result<Moved, Warning> {
    let personal = if personal { "--personal " } else { "" };
    let Some(path) = path else {
        return Err(Warning::new(
            None,
            "no mount is known for this merge: the change records none, or the personal board \
             mounts this checkout at no one path, so no mount was moved"
                .to_owned(),
            format!("board {personal}mount update /repos/<name>"),
        ));
    };
    let update = format!("board {personal}mount update {path}");
    let left = |cause: Option<String>, why: String, command: &str| {
        Warning::new(
            cause,
            format!("{why}, so {path} was left at its pin"),
            command.to_owned(),
        )
    };
    let logical: LogicalPath =
        (path.parse()).map_err(|_| left(None, format!("{path} is not a logical path"), &update))?;
    let report = (board.refs(&[logical.clone()], Absent::Refuse, &[]))
        .and_then(|answers| (answers.into_iter().next()).expect("refs answers each path asked"))
        .map_err(|error| {
            let refusal = error.refusal();
            let why = format!("{path} could not be read: {}", refusal.message);
            left(Some(error.code().as_str().to_owned()), why, &update)
        })?;
    let location = &report.location;
    let Some(follow) = (location.pointers.last()).and_then(|pointer| pointer.follow.as_deref())
    else {
        return Err(left(None, format!("{path} is not a mount"), &update));
    };
    let repository = &location.board.repository;
    let failed = |error: PluginError, command: &str| left(Some(error.code), error.message, command);
    let remotes =
        git::git(host, repository, &["remote"], "").map_err(|error| failed(error, &update))?;
    let Some((remote, branch)) = upstream(&remotes, follow) else {
        let why = format!(
            "{path} follows {follow}, a branch of no remote of {}, which a merge on GitHub does \
             not move",
            repository.display()
        );
        return Err(left(None, why, &update));
    };
    let command = fetch(repository, remote, branch, &update);
    if queued {
        let why = "GitHub only queued the merge, which has not landed yet".to_owned();
        return Err(left(None, why, &command));
    }
    git::git(host, repository, &["fetch", remote, branch], "")
        .map_err(|error| failed(error, &command))?;
    let report = (board.mount_update(&logical, LOCK_WAIT)).map_err(|error| {
        let why = format!("`board mount update` refused: {error}");
        left(Some(error.code().as_str().to_owned()), why, &command)
    })?;
    Ok(Moved {
        path: report.path.to_string(),
        old: report.old.to_string(),
        new: report.new.to_string(),
        wrote: report.wrote,
    })
}

/// The remote among `remotes`, one name a line, whose branch `follow`
/// names, and that branch: the longest name `follow` starts with, then a
/// `/`. None when `follow` is a local branch.
fn upstream<'a>(remotes: &'a str, follow: &'a str) -> Option<(&'a str, &'a str)> {
    let follow = follow.strip_prefix("refs/remotes/").unwrap_or(follow);
    (remotes.lines())
        .filter_map(|remote| {
            let branch = follow.strip_prefix(remote)?.strip_prefix('/')?;
            Some((remote, branch))
        })
        .max_by_key(|(remote, _)| remote.len())
}

/// `git -C <repository> fetch <remote> <branch> && <update>`.
fn fetch(repository: &Path, remote: &str, branch: &str, update: &str) -> String {
    let at = repository.display();
    format!("git -C {at} fetch {remote} {branch} && {update}")
}

impl Warning {
    fn new(cause: Option<String>, message: String, command: String) -> Self {
        Self {
            code: CODE.to_owned(),
            cause,
            message,
            command,
        }
    }

    /// The warning as Board prints it, naming the board to run the
    /// command in unless that is the personal board.
    fn refusal(&self, personal: bool) -> Refusal {
        let plugin: PluginName = "github".parse().expect("github is a plugin name");
        let code = PluginCode::parse(&plugin, CODE.to_owned()).expect("CODE is github's");
        let mut fields = Map::new();
        fields.insert("command".to_owned(), json!(self.command));
        if let Some(cause) = &self.cause {
            fields.insert("cause".to_owned(), json!(cause));
        }
        let mut refusal = Refusal::plugin(code, self.message.clone(), fields);
        let board = if personal {
            String::new()
        } else {
            format!(" {SAME_BOARD}")
        };
        refusal.hint = Some(format!("run `{}`{board}", self.command));
        refusal
    }
}
