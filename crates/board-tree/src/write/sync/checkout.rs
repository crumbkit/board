//! The checkout record, `board-checkout` in the git directory: the commits
//! a checkout was moving the working tree to. A checkout writes the files
//! of its changes before it moves HEAD, so a kill, or a HEAD that another
//! program moved, leaves files that hold the new commit's blobs under the
//! old HEAD. The next sync reads the record and accepts those files as its
//! own writes (docs/architecture/writes/sync/checkout.md).

use std::fs;
use std::path::PathBuf;

use board_model::ObjectId;

use super::super::{Lock, WriteError, write_temporary};
use crate::git::{Repository, model_id};

impl Repository {
    fn checkout_record(&self) -> PathBuf {
        self.git_dir().join("board-checkout")
    }

    /// The commits an earlier checkout was moving to, oldest first, that
    /// the object store still holds. An entry it does not hold is dropped.
    /// When HEAD is at the newest, that checkout finished its branch move
    /// and only its record was left, so the record is removed and none are
    /// returned. The board lock must be held.
    pub(super) fn recorded_checkouts(
        &self,
        lock: &Lock,
        head: Option<ObjectId>,
    ) -> Result<Vec<ObjectId>, WriteError> {
        let Ok(text) = fs::read_to_string(self.checkout_record()) else {
            return Ok(Vec::new());
        };
        let lines = text.lines().count();
        let readable: Vec<ObjectId> = text
            .lines()
            .filter_map(|line| gix::ObjectId::from_hex(line.trim().as_bytes()).ok())
            .filter(|id| self.git().find_commit(*id).is_ok())
            .map(|id| model_id(&id))
            .collect();
        if readable.last() == head.as_ref() || readable.is_empty() {
            self.clear_checkout(lock)?;
            return Ok(Vec::new());
        }
        if readable.len() != lines {
            self.record_checkout(lock, &readable)?;
        }
        Ok(readable)
    }

    /// Replaces the record with `commits`, by temporary file, fsync and
    /// rename, and syncs the directory. The board lock must be held.
    pub(super) fn record_checkout(
        &self,
        _lock: &Lock,
        commits: &[ObjectId],
    ) -> Result<(), WriteError> {
        let record = self.checkout_record();
        let text: String = commits
            .iter()
            .map(|commit| commit.to_string() + "\n")
            .collect();
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
