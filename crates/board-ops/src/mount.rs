//! `board mount add`: writing a mount of a local clone, and `board mount
//! update`: advancing a mount to the commit its `follow` ref names
//! (docs/architecture.md, Writes).

use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use board_model::{BINARY_NAME, Code, Fields, LogicalPath, Mount, ObjectId, Refusal};
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
        let put = self
            .put(path, Expect::Absent, record.to_string().as_bytes())
            .map_err(|error| match Taken::of(path, &error) {
                Some(taken) => MountError::Taken {
                    path: path.clone(),
                    taken,
                    source: Box::new(error),
                },
                None => error.into(),
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

/// What already holds the path `mount add` was given.
#[derive(Debug, Clone, Copy)]
pub enum Taken {
    /// A file holding this blob, such as a record.
    File(ObjectId),
    /// A mount or link record.
    Mount,
    /// A directory, or another entry with the path's name.
    Name,
}

impl Taken {
    /// What holds `path`, when `error`, put's refusal of a new file there,
    /// says something does.
    fn of(path: &LogicalPath, error: &PutError) -> Option<Self> {
        match error {
            PutError::Stale {
                found: Some(blob), ..
            } => Some(Self::File(*blob)),
            PutError::Write(WriteError::ThroughMount { mount, .. }) if mount == path => {
                Some(Self::Mount)
            }
            _ if error.code() == Code::PathShadowed => Some(Self::Name),
            _ => None,
        }
    }

    /// The commands that move what holds `path` aside. Board has no command
    /// that moves an entry, so they copy and remove it.
    fn hint(self, path: &LogicalPath) -> String {
        let moves = match self {
            Self::File(blob) => format!(
                "move the record at {path} aside and run mount add again: copy it with `board get {path} | board put <new path> --expect none`, then remove it with `board rm {path} --expect {blob}`"
            ),
            Self::Mount => format!(
                "move the mount at {path} aside and run mount add again: `board rm {path}` removes it and leaves its clone, then `board mount add <new path> <its clone>` mounts the clone there"
            ),
            Self::Name => format!(
                "clear {path} and run mount add again: `board ls {}` shows what already has its name, and `board rm -r {path}` removes a directory, since no command moves one",
                path.parent()
            ),
        };
        format!("mount add writes only where nothing is; mount at another path, or {moves}")
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
    /// Put refused the record because something already holds `path`.
    #[error("{source}")]
    Taken {
        path: LogicalPath,
        taken: Taken,
        source: Box<PutError>,
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
            Self::Taken { source, .. } => source.code(),
        }
    }

    pub fn refusal(&self) -> Refusal {
        let fields = match self {
            Self::Mounting { path, .. } => Fields::at(path),
            Self::Unfollowed { .. } => Fields::default(),
            Self::Pin(error) => error.fields(),
            Self::Put(error) => error.fields(),
            Self::Taken { source, .. } => source.fields(),
        };
        let refusal = Refusal::new(self.code(), self, &fields);
        match self {
            Self::Unfollowed { .. } => refusal.with_hint(
                "give the ref to follow with --follow, such as --follow main, or make the first commit in the repository",
            ),
            Self::Taken { path, taken, .. } => refusal.with_hint(taken.hint(path)),
            _ => refusal,
        }
    }
}

#[cfg(test)]
mod tests;
