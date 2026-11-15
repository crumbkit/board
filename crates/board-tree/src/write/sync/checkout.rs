//! The checkout record, `board-checkout` in the git directory: for each
//! checkout, the commit it started from and the commit it was moving the
//! working tree to. A checkout writes the files of its changes before it
//! moves HEAD, so a kill, or a HEAD that another program moved, leaves files
//! that hold the new commit's blobs under the old HEAD. The next sync reads
//! the record and accepts such a file as its own write at each path where
//! the two commits differ, which are the paths the checkout wrote
//! (docs/architecture/writes/sync/checkout.md).

use std::fs;
use std::path::PathBuf;

use board_model::ObjectId;

use super::super::{Lock, WriteError, write_temporary};
use crate::git::{Repository, model_id};

/// The commit a recorded checkout started from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Started {
    /// A record of the earlier format, which holds no start commit. No blob
    /// of its target commit is accepted.
    Unknown,
    /// The checkout started with no commit.
    Unborn,
    At(ObjectId),
}

/// One checkout of the record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Recorded {
    pub(super) started: Started,
    pub(super) target: ObjectId,
}

impl Recorded {
    /// The line of the record: the start commit, `-` for none, and the
    /// target; the earlier format holds the target alone.
    fn line(&self) -> String {
        match self.started {
            Started::Unknown => format!("{}\n", self.target),
            Started::Unborn => format!("- {}\n", self.target),
            Started::At(commit) => format!("{commit} {}\n", self.target),
        }
    }
}

impl Repository {
    fn checkout_record(&self) -> PathBuf {
        self.git_dir().join("board-checkout")
    }

    /// Whether the object store holds the commit `id`.
    fn holds(&self, id: gix::ObjectId) -> bool {
        self.git().find_commit(id).is_ok()
    }

    /// The checkouts an earlier sync was making, oldest first, whose target
    /// commits the object store still holds. An entry it does not hold is
    /// dropped, and a start commit it does not hold is unknown. When HEAD is
    /// at the newest target, that checkout finished its branch move and only
    /// its record was left, so the record is removed and none are returned.
    /// The board lock must be held.
    pub(super) fn recorded_checkouts(
        &self,
        lock: &Lock,
        head: Option<ObjectId>,
    ) -> Result<Vec<Recorded>, WriteError> {
        let Ok(text) = fs::read_to_string(self.checkout_record()) else {
            return Ok(Vec::new());
        };
        let lines = text.lines().count();
        let id = |text: &str| gix::ObjectId::from_hex(text.trim().as_bytes()).ok();
        let readable: Vec<Recorded> = text
            .lines()
            .filter_map(|line| {
                let (started, target) = match line.split_once(' ') {
                    None => (Started::Unknown, line),
                    Some(("-", target)) => (Started::Unborn, target),
                    Some((start, target)) => (
                        id(start)
                            .filter(|start| self.holds(*start))
                            .map_or(Started::Unknown, |start| Started::At(model_id(&start))),
                        target,
                    ),
                };
                let target = id(target).filter(|target| self.holds(*target))?;
                Some(Recorded {
                    started,
                    target: model_id(&target),
                })
            })
            .collect();
        if readable.last().map(|last| last.target) == head || readable.is_empty() {
            self.clear_checkout(lock)?;
            return Ok(Vec::new());
        }
        if readable.len() != lines {
            self.record_checkout(lock, &readable)?;
        }
        Ok(readable)
    }

    /// Replaces the record with `checkouts`, by temporary file, fsync and
    /// rename, and syncs the directory. The board lock must be held.
    pub(super) fn record_checkout(
        &self,
        _lock: &Lock,
        checkouts: &[Recorded],
    ) -> Result<(), WriteError> {
        let record = self.checkout_record();
        let text: String = checkouts.iter().map(Recorded::line).collect();
        let io = |source| WriteError::Io {
            path: record.clone(),
            source,
        };
        let temporary = record.with_extension("tmp");
        write_temporary(&temporary, &record, text.as_bytes())
            .and_then(|()| fs::rename(&temporary, &record))
            .map_err(io)?;
        fs::File::open(self.git_dir())
            .and_then(|directory| directory.sync_all())
            .map_err(io)
    }

    /// Removes the record, which is done when it is gone. The board lock
    /// must be held.
    pub(super) fn clear_checkout(&self, _lock: &Lock) -> Result<(), WriteError> {
        let record = self.checkout_record();
        match fs::remove_file(&record) {
            Err(source) if source.kind() != std::io::ErrorKind::NotFound => Err(WriteError::Io {
                path: record,
                source,
            }),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests;
