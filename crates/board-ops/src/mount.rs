//! `board mount add`: writing a mount of a local clone, and `board mount
//! update`: advancing a mount to the commit its `follow` ref names
//! (docs/architecture.md, Writes).

use std::time::Duration;

use board_model::{BINARY_NAME, Code, Fields, LogicalPath, Mount, ObjectId, Refusal};
use board_tree::{LOCK_WAIT, Lock, Pin, PinError, Target};

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
    /// Writes at `path`, where nothing is, a `board/mount/v1` record of the
    /// repository at `repo`, a path as the record holds it, following
    /// `follow` or else the branch checked out there, at the commit that
    /// ref names now, found as `mount update` finds it. Returns the mount
    /// and put's report of its record.
    pub fn mount_add(
        &self,
        path: &LogicalPath,
        repo: &str,
        follow: Option<String>,
    ) -> Result<(Mount, PutReport), MountError> {
        let repositories = self.repositories();
        let mounting = repositories.mounting(repositories.root().path(), repo, follow);
        let refused = |code| MountError::Mounting {
            path: path.clone(),
            repo: repo.to_owned(),
            code,
        };
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
        let put = self.put(path, Expect::Absent, record.to_string().as_bytes())?;
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
        }
    }

    pub fn refusal(&self) -> Refusal {
        let fields = match self {
            Self::Mounting { path, .. } => Fields::at(path),
            Self::Unfollowed { .. } => Fields::default(),
            Self::Pin(error) => error.fields(),
            Self::Put(error) => error.fields(),
        };
        let refusal = Refusal::new(self.code(), self, &fields);
        match self {
            Self::Unfollowed { .. } => refusal.with_hint(
                "give the ref to follow with --follow, such as --follow main, or make the first commit in the repository",
            ),
            _ => refusal,
        }
    }
}

#[cfg(test)]
mod tests;
