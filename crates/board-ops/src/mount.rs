//! `board mount add`: writing a mount of a local clone, and `board mount
//! update`: advancing a mount to the commit its `follow` ref names
//! (docs/architecture.md, Writes).

use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use board_model::{
    BINARY_NAME, Code, Fields, Found, LogicalPath, Mount, ObjectId, Pointer, Refusal,
};
use board_tree::{LOCK_WAIT, Lock, Pin, PinError, Target, WriteError};

use crate::board::Board;
use crate::put::{Expect, Failure, Prepared, PutError, PutReport, Schemas};

/// What `mount update` found and whether it wrote.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MountUpdateReport {
    pub path: LogicalPath,
    pub old: ObjectId,
    pub new: ObjectId,
    pub wrote: bool,
}

impl Board {
    /// Writes at `path`, which holds nothing yet, a `board/mount/v1` record
    /// of the repository at `repo`, read against `start`, following
    /// `follow` or else the branch checked out there, at the commit that
    /// ref names now, found as `mount update` finds it. A URL is refused as
    /// a mount record holding it is. Returns the mount and put's report of
    /// its record.
    pub fn mount_add(
        &self,
        path: &LogicalPath,
        (repo, start): (&str, &Path),
        follow: Option<String>,
    ) -> Result<(Mount, PutReport), MountError> {
        let repositories = self.repositories();
        let refused = |code| MountError::Mounting {
            path: path.clone(),
            repo: repo.to_owned(),
            code,
        };
        // The pointer parser decides what counts as a remote; the commit is
        // a stand-in, since only `repo` is judged.
        let probe = serde_json::json!({
            "$type": "board/mount/v1",
            "repo": repo,
            "follow": "main",
            "commit": ObjectId::Sha1([0; 20]).to_string(),
        });
        if let Err(error) = board_model::pointer(probe.to_string().as_bytes()) {
            return Err(refused(error.code()));
        }
        let repo = &stored(repo, start);
        let mounting = repositories.mounting(repositories.root().path(), repo, follow);
        let mount = mounting
            .map_err(refused)?
            .ok_or_else(|| MountError::Unfollowed {
                repo: repo.to_owned(),
            })?;
        let record = serde_json::json!({
            "$type": "board/mount/v1",
            "repo": mount.repo,
            "follow": mount.follow,
            "commit": mount.commit.to_string(),
        });
        // Put follows a link at `path`, so a pointer there is named first.
        if let Ok(Some((_, held, pointer))) = repositories.pointer_at(path) {
            let follow = match &pointer {
                Pointer::Mount(held) => {
                    let mounting =
                        repositories.mounting(repositories.root().path(), &held.repo, None);
                    let default = mounting.ok().flatten().map(|mount| mount.follow);
                    (default.as_ref() != Some(&held.follow)).then(|| held.follow.clone())
                }
                Pointer::Link(_) => None,
            };
            return Err(MountError::Pointed {
                path: path.clone(),
                blob: held.blob,
                pointer: Box::new(pointer),
                follow,
            });
        }
        let put = self
            .put(path, Expect::Absent, record.to_string().as_bytes())
            .map_err(|error| {
                let taken = match &error {
                    PutError::Stale { found, .. } => found.is_some(),
                    PutError::Write(WriteError::Degraded { path: at, code }) => {
                        at == path && *code == Code::PathShadowed
                    }
                    _ => false,
                };
                if taken {
                    MountError::Taken(Box::new(error))
                } else {
                    error.into()
                }
            })?;
        Ok((mount, put))
    }

    /// Rewrites the mount pointer at `path`, which is not followed, to the
    /// commit its `follow` ref names now, through put with the pointer's
    /// blob as the expect. The pointer is read under the lock the write
    /// holds, which it waits for up to `wait` or [`LOCK_WAIT`], whichever
    /// is shorter. A mount already there is left as it is.
    pub fn mount_update(
        &self,
        path: &LogicalPath,
        wait: Duration,
    ) -> Result<MountUpdateReport, MountError> {
        let lock = self
            .repositories()
            .lock(wait.min(LOCK_WAIT))
            .map_err(|error| MountError::Put(error.into()))?;
        let Pin {
            target,
            blob,
            old,
            new,
            canonical,
            ..
        } = self.repositories().pin(path)?;
        let mut report = MountUpdateReport {
            path: path.clone(),
            old,
            new,
            wrote: false,
        };
        if new == old {
            return Ok(report);
        }
        // The new pin brings every file the mounted commits changed, so the
        // commit is marked as a bulk one.
        let reason = format!("mount update {path}");
        let message = format!("{BINARY_NAME} {reason}");
        let message = match crate::Bulk::new(&reason) {
            Ok(bulk) => bulk.mark(&message),
            Err(_) => message,
        };
        self.repin(lock, path, target, blob, &canonical, &message)?;
        report.wrote = true;
        Ok(report)
    }

    /// Rewrites the mount pointer at `path`, found at `target` holding
    /// `blob` under `lock`, to `canonical`, through put with that blob as
    /// the expect.
    pub(crate) fn repin(
        &self,
        lock: Lock,
        path: &LogicalPath,
        target: Target,
        blob: ObjectId,
        canonical: &[u8],
        message: &str,
    ) -> Result<PutReport, PutError> {
        let admitted = self
            .check_record(&mut Schemas::default(), canonical)
            .map_err(|refused| PutError::Record {
                path: path.clone(),
                refused,
            })?;
        let prepared = Prepared {
            path: path.clone(),
            expect: Expect::Blob(blob),
            canonical: canonical.to_vec(),
            target,
            admitted,
        };
        self.write_all(lock, &[prepared], &[], message)
            .map_err(|failure| match failure {
                Failure::Stale(mut stale) => stale.remove(0).1,
                Failure::Error(error) => error,
            })
    }
}

/// `repo` as a mount record stores it: as given when it starts with `~/`;
/// else absolute, read against `start`, the directory the run started in,
/// with each `.` and `..` taken out.
fn stored(repo: &str, start: &Path) -> String {
    if repo.starts_with("~/") {
        return repo.to_owned();
    }
    let mut clean = PathBuf::new();
    for component in start.join(repo).components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                clean.pop();
            }
            other => clean.push(other),
        }
    }
    clean.to_string_lossy().into_owned()
}

/// What holds a path `mount add` was given, for its refusal's message.
fn held(error: &PutError) -> String {
    match error {
        PutError::Stale { path, .. } => format!("{path} already holds a record"),
        PutError::Write(WriteError::Degraded { path, .. }) => {
            format!("{path} would share its name with another entry beside it")
        }
        _ => error.to_string(),
    }
}

/// What the pointer at a path `mount add` was given leads to.
fn pointing(pointer: &Pointer) -> String {
    match pointer {
        Pointer::Link(to) => format!("a link to {to}"),
        Pointer::Mount(mount) => format!("a mount of {}", mount.repo),
    }
}

/// `word` as a shell reads it back: as it is when it holds only characters
/// a shell leaves alone, else in single quotes, so `~/` stays unexpanded.
fn quoted(word: &str) -> String {
    let plain = |c: char| c.is_ascii_alphanumeric() || "/._-+,:@=".contains(c);
    if word.chars().all(plain) {
        word.to_owned()
    } else {
        format!("'{}'", word.replace('\'', r"'\''"))
    }
}

/// A mount update that wrote nothing, or, for a put's `head_moved`, wrote
/// but did not commit.
#[derive(Debug, thiserror::Error)]
pub enum MountError {
    #[error("{repo} cannot be mounted at {path}: {code}")]
    Mounting {
        path: LogicalPath,
        repo: String,
        code: Code,
    },
    #[error(
        "{repo} has no branch with a commit checked out, so `mount add` cannot choose what to follow"
    )]
    Unfollowed { repo: String },
    /// Put refused the record because a record or a directory already
    /// holds the path.
    #[error("{}, and mount add writes only where nothing is", held(.0))]
    Taken(Box<PutError>),
    /// A link or a mount, at `blob`, already holds `path`. `follow` is the
    /// mount's, when it is not the branch checked out in its clone.
    #[error(
        "{path} already holds {}, and mount add writes only where nothing is",
        pointing(pointer)
    )]
    Pointed {
        path: LogicalPath,
        blob: ObjectId,
        pointer: Box<Pointer>,
        follow: Option<String>,
    },
    #[error(transparent)]
    Pin(#[from] PinError),
    #[error(transparent)]
    Put(#[from] PutError),
}

impl MountError {
    pub fn code(&self) -> Code {
        match self {
            Self::Mounting { code, .. } => *code,
            Self::Unfollowed { .. } => Code::CliUsage,
            Self::Pin(error) => error.code(),
            Self::Put(error) => error.code(),
            Self::Taken(error) => error.code(),
            Self::Pointed { .. } => Code::PutStale,
        }
    }

    pub fn refusal(&self) -> Refusal {
        let fields = match self {
            Self::Mounting { path, .. } => Fields::at(path),
            Self::Unfollowed { .. } => Fields::default(),
            Self::Pin(error) => error.fields(),
            Self::Put(error) => error.fields(),
            Self::Taken(error) => error.fields(),
            Self::Pointed { path, blob, .. } => Fields {
                blob: Some(*blob),
                found: Some(Found::Blob(*blob)),
                ..Fields::at(path)
            },
        };
        let refusal = Refusal::new(self.code(), self, &fields);
        match self {
            Self::Unfollowed { .. } => refusal.with_hint(
                "give the ref to follow with --follow, such as --follow main, or make the first commit in the repository",
            ),
            Self::Taken(error) => match &**error {
                PutError::Stale {
                    path,
                    found: Some(blob),
                } => refusal.with_hint(format!(
                    "mount at another path, or move the record aside and run mount add again: copy it and, once the copy succeeds, remove it with `board get {path} | board put <new path> --expect none && board rm {path} --expect {blob}`"
                )),
                PutError::Write(WriteError::Degraded { path, .. }) => refusal.with_hint(format!(
                    "mount at another path, or clear {path} and run mount add again: `board ls {}` shows what already has its name, and `board rm -r {path}` removes a directory, since no command moves one",
                    path.parent()
                )),
                _ => refusal,
            },
            Self::Pointed {
                path,
                blob,
                pointer,
                follow,
            } => refusal.with_hint(match &**pointer {
                Pointer::Link(to) => format!(
                    r#"mount at another path, or move the link aside and run mount add again: `board rm {path} --expect {blob}` removes the link and leaves {to}, then `echo '{{"$type": "board/mount/v1", "path": "{to}"}}' | board put <new path> --expect none` links {to} there"#
                ),
                Pointer::Mount(mount) => {
                    let follow = follow.as_ref().map_or_else(String::new, |follow| {
                        format!(" --follow {}", quoted(follow))
                    });
                    format!(
                        "mount at another path, or move the mount aside and run mount add again: `board rm {path} --expect {blob}` removes it and leaves its clone, then `board mount add <new path> {}{follow}` mounts the clone there",
                        quoted(&mount.repo)
                    )
                }
            }),
            _ => refusal,
        }
    }
}

#[cfg(test)]
mod tests;
