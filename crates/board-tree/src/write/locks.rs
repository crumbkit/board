//! The locks a put holds: `board.lock` and git's `index.lock`
//! (docs/architecture.md, Writes).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gix::lock::acquire::Fail;

use board_model::ObjectId;

use super::{LOCK_POLL, Lock, Staged, WriteError};
use crate::git::Repository;

mod owner;

pub(super) use owner::Owned;
use owner::clear;

impl Repository {
    /// The lock owner record, which lists the git lock files Board holds.
    fn owners(&self) -> PathBuf {
        self.git_dir().join("board-locks")
    }

    /// Takes `board.lock` in the git directory with gix's lock files, trying
    /// again every `LOCK_POLL` until `wait` has passed. The returned lock
    /// carries the deadline `wait` after now, which `index.lock` waits share.
    ///
    /// A kernel lock on `board.flock` is taken first and held as long as the
    /// board lock is. The kernel drops it when the process ends, however it
    /// ends, so while this process holds it no other Board process holds
    /// `board.lock`, `index.lock` or a branch lock. Each of those files
    /// that the lock owner record names, with the device and inode the
    /// record holds, is then the leftover of a killed Board process, and is
    /// removed. Any other lock file is waited for and then refused, naming
    /// the file.
    pub fn lock(&self, wait: Duration) -> Result<Lock, WriteError> {
        let resource = self.git_dir().join("board");
        let marker = resource.with_extension("lock");
        let started = Instant::now();
        let deadline = started + wait;
        let flock = Self::flock(&resource.with_extension("flock"), &marker, wait)?;
        clear(&self.owners()).map_err(|(path, source)| WriteError::Io { path, source })?;
        loop {
            match gix::lock::Marker::acquire_to_hold_resource(&resource, Fail::Immediately, None) {
                Ok(held) => {
                    return Ok(Lock {
                        _held: held,
                        _owner: Owned::claim(&self.owners(), &[&marker]),
                        _flock: flock,
                        deadline,
                    });
                }
                Err(error) if error.is_retryable() && started.elapsed() < wait => {
                    std::thread::sleep(LOCK_POLL);
                }
                Err(error) => return Err(lock_refused(error, marker)),
            }
        }
    }

    /// Takes an exclusive kernel lock on the file `flock`, trying again every
    /// `LOCK_POLL` until `wait` has passed, then refuses with
    /// [`WriteError::Locked`] naming `marker`, the board lock it stands for.
    fn flock(flock: &Path, marker: &Path, wait: Duration) -> Result<fs::File, WriteError> {
        let failed = |source| WriteError::Io {
            path: flock.to_path_buf(),
            source,
        };
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(flock)
            .map_err(failed)?;
        let started = Instant::now();
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(file),
                Err(fs::TryLockError::WouldBlock) if started.elapsed() < wait => {
                    std::thread::sleep(LOCK_POLL);
                }
                Err(fs::TryLockError::WouldBlock) => {
                    return Err(WriteError::Locked {
                        lock: marker.to_path_buf(),
                    });
                }
                Err(fs::TryLockError::Error(source)) => return Err(failed(source)),
            }
        }
    }

    /// Takes git's `index.lock`, trying again every `LOCK_POLL` until
    /// `deadline` has passed, then reads git's index for this working tree, or an empty
    /// one before the first `git add`. A lock file that a killed Board
    /// process left is removed first. The returned owner keeps this
    /// process's lock in the lock owner record until it is dropped.
    pub(super) fn lock_index(
        &self,
        held: &Lock,
    ) -> Result<(gix::lock::File, gix::index::File, Owned), WriteError> {
        let deadline = held.deadline;
        let git = self.git();
        let index = git.index_path();
        let file = index.with_extension("lock");
        let lock = loop {
            match gix::lock::File::acquire_to_update_resource(&index, Fail::Immediately, None) {
                Ok(lock) => break lock,
                Err(error) if error.is_retryable() && Instant::now() < deadline => {
                    std::thread::sleep(LOCK_POLL);
                }
                Err(error) => return Err(lock_refused(error, file)),
            }
        };
        let owner = Owned::claim(&self.owners(), &[&file]);
        let read = if index.exists() {
            git.open_index().map_err(|error| WriteError::Commit {
                source: error.into(),
            })?
        } else {
            gix::index::File::from_state(gix::index::State::new(git.object_hash()), index)
        };
        Ok((lock, read, owner))
    }

    /// The edit that moves HEAD's branch to the commit `staged` holds, from
    /// the commit it was staged on, with `log` as the reflog message.
    pub(super) fn head_edit(
        staged: &Staged,
        log: &str,
    ) -> Result<gix::refs::transaction::RefEdit, WriteError> {
        use gix::refs::Target as RefTarget;
        use gix::refs::transaction::{Change, LogChange, PreviousValue, RefEdit, RefLog};
        let id = |commit: ObjectId| {
            gix::ObjectId::try_from(commit.as_bytes()).map_err(|error| WriteError::Commit {
                source: error.into(),
            })
        };
        let expected = match staged.parent {
            Some(parent) => PreviousValue::MustExistAndMatch(RefTarget::Object(id(parent)?)),
            None => PreviousValue::MustNotExist,
        };
        let name = "HEAD"
            .try_into()
            .map_err(
                |error: gix::validate::reference::name::Error| WriteError::Commit {
                    source: error.into(),
                },
            )?;
        Ok(RefEdit {
            change: Change::Update {
                log: LogChange {
                    mode: RefLog::AndReference,
                    force_create_reflog: false,
                    message: log.into(),
                },
                expected,
                new: RefTarget::Object(id(staged.commit)?),
            },
            name,
            deref: true,
        })
    }

    /// Locks the branch as [`Self::lock_branch`] does, and records that
    /// this process holds its lock file.
    pub(super) fn claim_branch<'a>(
        &self,
        held: &Lock,
        git: &'a gix::Repository,
        edit: &gix::refs::transaction::RefEdit,
    ) -> Result<(Owned, gix::refs::file::Transaction<'a, 'a>), WriteError> {
        // The transaction locks HEAD's reflog beside the branch itself.
        let mut locks = vec![git.git_dir().join("HEAD.lock")];
        if let Some(branch) = git.head_name().ok().flatten() {
            locks.push(git.common_dir().join(format!("{branch}.lock")));
        }
        let prepared = Self::lock_branch(git, edit, held.deadline)?;
        let locks: Vec<&Path> = locks.iter().map(PathBuf::as_path).collect();
        let owner = Owned::claim(&self.owners(), &locks);
        Ok((owner, prepared))
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
