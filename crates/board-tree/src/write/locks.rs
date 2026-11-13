//! The locks a put holds: `board.lock` and git's `index.lock`
//! (docs/architecture.md, Writes).

use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use gix::lock::acquire::Fail;

use super::{LOCK_POLL, Lock, WriteError};
use crate::git::Repository;

impl Repository {
    /// Takes `board.lock` in the git directory with gix's lock files, trying
    /// again every `LOCK_POLL` until `wait` has passed. The returned lock
    /// carries the deadline `wait` after now, which `index.lock` waits share.
    pub fn lock(&self, wait: Duration) -> Result<Lock, WriteError> {
        let resource = self.git_dir().join("board");
        let started = Instant::now();
        let deadline = started + wait;
        loop {
            match gix::lock::Marker::acquire_to_hold_resource(&resource, Fail::Immediately, None) {
                Ok(held) => {
                    return Ok(Lock {
                        _held: held,
                        deadline,
                    });
                }
                Err(error) if error.is_retryable() && started.elapsed() < wait => {
                    std::thread::sleep(LOCK_POLL);
                }
                Err(error) => return Err(lock_refused(error, resource.with_extension("lock"))),
            }
        }
    }

    /// Takes git's `index.lock`, trying again every `LOCK_POLL` until
    /// `deadline` has passed, then reads git's index for this working tree, or an empty
    /// one before the first `git add`.
    pub(super) fn lock_index(
        &self,
        deadline: Instant,
    ) -> Result<(gix::lock::File, gix::index::File), WriteError> {
        let git = self.git();
        let index = git.index_path();
        let lock = loop {
            match gix::lock::File::acquire_to_update_resource(&index, Fail::Immediately, None) {
                Ok(lock) => break lock,
                Err(error) if error.is_retryable() && Instant::now() < deadline => {
                    std::thread::sleep(LOCK_POLL);
                }
                Err(error) => return Err(lock_refused(error, index.with_extension("lock"))),
            }
        };
        let read = if index.exists() {
            git.open_index().map_err(|error| WriteError::Commit {
                source: error.into(),
            })?
        } else {
            gix::index::File::from_state(gix::index::State::new(git.object_hash()), index)
        };
        Ok((lock, read))
    }
}

impl Repository {
    /// Locks the branch `edit` updates and checks its expected value,
    /// trying again every `LOCK_POLL` until `deadline` while another process
    /// holds the lock. A branch that no longer holds the expected value is
    /// [`WriteError::HeadMoved`]; a lock still held at the deadline is
    /// [`WriteError::Locked`].
    pub(super) fn lock_branch<'a>(
        git: &'a gix::Repository,
        edit: &gix::refs::transaction::RefEdit,
        deadline: Instant,
    ) -> Result<gix::refs::file::Transaction<'a, 'a>, WriteError> {
        use gix::refs::file::transaction::prepare::{MustNotExist, ReferenceOutOfDate};
        loop {
            match git.refs.transaction().prepare(
                [edit.clone()],
                Fail::Immediately,
                Fail::Immediately,
            ) {
                Ok(prepared) => return Ok(prepared),
                Err(error) => {
                    if error.downcast_any_ref::<ReferenceOutOfDate>().is_some()
                        || error.downcast_any_ref::<MustNotExist>().is_some()
                    {
                        return Err(WriteError::HeadMoved);
                    }
                    if !error.is_retryable() {
                        return Err(WriteError::Uncommitted {
                            source: error.into(),
                        });
                    }
                    if Instant::now() >= deadline {
                        let branch = git
                            .head_name()
                            .ok()
                            .flatten()
                            .map_or_else(|| "HEAD".to_owned(), |name| name.to_string());
                        let lock = git.common_dir().join(format!("{branch}.lock"));
                        return Err(WriteError::Locked { lock });
                    }
                    std::thread::sleep(LOCK_POLL);
                }
            }
        }
    }
}

/// The refusal for a lock gix could not take: `Locked` when another holds
/// `lock`, and an I/O failure otherwise.
fn lock_refused(error: gix::Exn, lock: PathBuf) -> WriteError {
    if error.is_retryable() {
        WriteError::Locked { lock }
    } else {
        WriteError::Io {
            path: lock,
            source: io::Error::other(error.into_error()),
        }
    }
}
