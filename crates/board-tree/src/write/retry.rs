//! A put whose HEAD moved under it stages again (docs/architecture.md,
//! Writes).

use super::{Lock, Staged, WriteError};
use crate::git::{GitError, Repository, model_id};
use board_model::ObjectId;

/// How many times a put stages again when HEAD moved under it.
const HEAD_RETRIES: usize = 3;

impl Repository {
    /// Finishes `staged` as [`Self::finish_commit`] does, and when HEAD
    /// moved under it stages `writes` again on the new HEAD and finishes
    /// that, up to `HEAD_RETRIES` times. Before each retry it compares, for
    /// every path in `writes`, the blob the new HEAD holds with the blob
    /// the commit `staged` was made on held; a path that differs means the
    /// put would commit over a change its caller did not see, and the
    /// result is [`WriteError::HeadMoved`]. `None` when the new HEAD
    /// already holds every write.
    pub fn finish_commit_retrying(
        &self,
        lock: &Lock,
        staged: Staged,
        writes: &[(&str, Option<&[u8]>)],
        message: &str,
    ) -> Result<Option<ObjectId>, WriteError> {
        let seen = staged.parent;
        let mut staged = staged;
        for _ in 0..=HEAD_RETRIES {
            match self.finish_commit(lock, staged) {
                Err(WriteError::HeadMoved) => {}
                done => return done.map(Some),
            }
            let head = self.head()?;
            for (path, _) in writes {
                if self.blob_at(seen, path)? != self.blob_at(head, path)? {
                    return Err(WriteError::HeadMoved);
                }
            }
            match self.stage_commit(lock, writes, message)? {
                Some(again) => staged = again,
                None => return Ok(None),
            }
        }
        Err(WriteError::HeadMoved)
    }

    /// The blob the `/`-separated `file` holds at `commit`; `None` where it
    /// is absent or `commit` is `None`.
    fn blob_at(&self, commit: Option<ObjectId>, file: &str) -> Result<Option<ObjectId>, GitError> {
        let Some(commit) = commit else {
            return Ok(None);
        };
        let tree = self
            .git()
            .find_commit(self.gix_id(commit)?)
            .and_then(|found| found.tree())
            .map_err(|error| self.missing(commit, error))?;
        let entry = tree
            .lookup_entry_by_path(file)
            .map_err(|error| self.missing(commit, error))?;
        Ok(entry
            .filter(|entry| entry.mode().is_blob())
            .map(|entry| model_id(entry.oid())))
    }
}
