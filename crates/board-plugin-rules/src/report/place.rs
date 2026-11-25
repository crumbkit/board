//! Where the repository a report reads lies, the clone git reads there,
//! and what git says of it.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use board_model::{LogicalPath, repo_identity};
use board_ops::{Host, Origin, PluginError, Run};
use serde_json::{Map, Value};

use super::BoardRead;
use crate::check::under;
use crate::index;

/// How long one git command may run.
const GIT_TIME: Duration = Duration::from_secs(10);

/// Where the repository lies: the mount it is at, by display path, with
/// the commit read there, or the board's own tree; and the clone git reads
/// for each.
pub(crate) struct Place {
    /// The mount, or none for the board's own tree.
    pub(crate) mount: Option<(LogicalPath, String)>,
    /// The clone of the repository, when Board knows one.
    pub(crate) clone: Option<PathBuf>,
    /// The board's own working tree.
    board: Option<PathBuf>,
}

/// Each mount by display path, the commit read there, and the clone that
/// holds it.
const MOUNTED: &str = "SELECT e.logical, m.target, s.repository FROM mounts m \
    JOIN entries e ON e.source = m.source AND e.path = m.path \
    LEFT JOIN sources s ON s.source = m.target \
    WHERE m.target IS NOT NULL AND e.logical IS NOT NULL";

/// The board's own working tree.
const OWN_TREE: &str = "SELECT repository FROM sources WHERE source = 'worktree'";

impl Place {
    /// The deepest mount `scope` lies at or under, or the board's own tree.
    pub(crate) fn of(host: &dyn Host, scope: &LogicalPath) -> Result<Self, PluginError> {
        let rows = index::rows(host, MOUNTED)?.rows;
        let mut deepest: Option<(LogicalPath, String, Option<PathBuf>)> = None;
        for row in rows {
            let [Value::String(path), Value::String(commit), clone] = &row[..] else {
                continue;
            };
            let Ok(path) = path.parse::<LogicalPath>() else {
                continue;
            };
            let deeper = (deepest.as_ref())
                .is_none_or(|(found, _, _)| path.segments().count() > found.segments().count());
            if under(scope, &path) && deeper {
                let clone = clone.as_str().map(PathBuf::from);
                deepest = Some((path, commit.clone(), clone));
            }
        }
        let board: Option<PathBuf> = index::first_strings::<Vec<String>>(host, OWN_TREE)?
            .into_iter()
            .next()
            .map(PathBuf::from);
        Ok(match deepest {
            Some((path, commit, clone)) => Self {
                mount: Some((path, commit)),
                clone,
                board,
            },
            None => Self {
                mount: None,
                clone: board.clone(),
                board,
            },
        })
    }

    /// The repository's name: `given` in identity form, else the clone's
    /// origin, else, run from a checkout of the repository, its origin.
    pub(crate) fn name(&self, host: &dyn Host, given: Option<&str>) -> Result<String, PluginError> {
        let refuse = |why: String| PluginError {
            code: "rules::report::no_repository".to_owned(),
            message: format!("the report cannot name the repository: {why}"),
            fields: Map::new(),
            hint: Some(
                "pass --repository with the repository's name, such as owner/app".to_owned(),
            ),
        };
        if let Some(given) = given {
            return (repo_identity(given))
                .map(|remote| remote.as_str().to_owned())
                .map_err(|error| refuse(format!("--repository {error}")));
        }
        let mut origins = Vec::new();
        if let Some(clone) = &self.clone {
            origins.push(host.clone_origin(clone));
        }
        let checkout = match &self.mount {
            Some((mount, _)) => host.started_in().is_some_and(|start| start.mount == *mount),
            None => host.caller_in_board(),
        };
        if checkout {
            origins.push(host.caller_remote());
        }
        let mut why = "the repository's clone has no origin remote".to_owned();
        for origin in origins {
            match origin {
                Origin::Url(url) => match repo_identity(&url) {
                    Ok(remote) => return Ok(remote.as_str().to_owned()),
                    Err(error) => why = error.to_string(),
                },
                Origin::Unread(unread) => why = unread,
                Origin::Outside => {}
            }
        }
        Err(refuse(why))
    }

    /// The commit read: the mount's, or the own tree's HEAD.
    pub(crate) fn commit(&self, host: &dyn Host) -> Option<String> {
        match &self.mount {
            Some((_, commit)) => Some(commit.clone()),
            None => head(host, self.board.as_deref()?),
        }
    }

    /// When `commit` was made, as git in the clone reads it.
    pub(crate) fn committed(&self, host: &dyn Host, commit: &str) -> Option<String> {
        let clone = self.clone.as_deref()?;
        let ran = git(host, clone, &["log", "-1", "--format=%ct", commit])?;
        let seconds: u64 = ran.ok()?.trim().parse().ok()?;
        Some(board_model::utc_seconds(
            SystemTime::UNIX_EPOCH + Duration::from_secs(seconds),
        ))
    }

    /// Whether a tracked file in the checkout read differs from its commit:
    /// the checkout Board ran from when it is the mount's, or the board's
    /// own working tree.
    pub(crate) fn uncommitted(&self, host: &dyn Host) -> bool {
        match &self.mount {
            Some((mount, _)) => (host.started_in())
                .filter(|start| start.mount == *mount)
                .and_then(|start| start.uncommitted())
                .unwrap_or(false),
            None => (self.board.as_deref())
                .and_then(|board| {
                    git(
                        host,
                        board,
                        &["status", "--porcelain", "--untracked-files=no"],
                    )
                })
                .and_then(Ran::ok)
                .is_some_and(|status| !status.trim().is_empty()),
        }
    }

    /// The Board that runs and the rules board's origin and HEAD.
    pub(crate) fn board_read(&self, host: &dyn Host) -> BoardRead {
        let board = self.board.as_deref();
        let rules_from = board.and_then(|board| match host.clone_origin(board) {
            Origin::Url(url) => repo_identity(&url)
                .ok()
                .map(|remote| remote.as_str().to_owned()),
            Origin::Outside | Origin::Unread(_) => None,
        });
        BoardRead {
            version: board_ops::running_release(),
            rules_from,
            rules_commit: board.and_then(|board| head(host, board)),
        }
    }

    /// Each of `paths` from the repository's root.
    pub(crate) fn all_relative(&self, paths: &[String]) -> Vec<String> {
        paths.iter().map(|path| self.relative(path)).collect()
    }

    /// `path`, a display path in the board, from the repository's root.
    pub(crate) fn relative(&self, path: &str) -> String {
        let Some((mount, _)) = &self.mount else {
            return path.to_owned();
        };
        match path.strip_prefix(mount.as_str()) {
            Some("") => "/".to_owned(),
            Some(rest) if rest.starts_with('/') => rest.to_owned(),
            _ => path.to_owned(),
        }
    }
}

/// What git printed and whether it exited 0.
pub(crate) struct Ran {
    pub(crate) code: Option<i32>,
    pub(crate) stdout: String,
}

impl Ran {
    pub(crate) fn ok(self) -> Option<String> {
        (self.code == Some(0)).then_some(self.stdout)
    }
}

/// `git -C <at> <arguments>` through the host, or none when git could not
/// run.
pub(crate) fn git(host: &dyn Host, at: &Path, arguments: &[&str]) -> Option<Ran> {
    let at = at.to_string_lossy();
    let arguments: Vec<String> = (["-C", &at].into_iter())
        .chain(arguments.iter().copied())
        .map(str::to_owned)
        .collect();
    let exited = host
        .run(&Run {
            program: "git",
            arguments: &arguments,
            directory: None,
            input: &[],
            deadline: GIT_TIME,
            added: &[],
        })
        .ok()?;
    Some(Ran {
        code: exited.code,
        stdout: String::from_utf8_lossy(&exited.stdout).into_owned(),
    })
}

/// The commit HEAD names in the repository at `at`.
fn head(host: &dyn Host, at: &Path) -> Option<String> {
    let id = git(host, at, &["rev-parse", "HEAD"])?.ok()?;
    let id = id.trim();
    board_model::ObjectId::from_hex(id).map(|_| id.to_owned())
}
