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
    /// that, up to `HEAD_RETRIES` times. `seen` is the HEAD the put checked
    /// its `--expect` against. Before each finish, for every path in
    /// `writes`, the blob the commit being finished is built on must be the
    /// one `seen` held: otherwise the put would commit over a change its
    /// caller did not see, and the result is [`WriteError::HeadMoved`], as
    /// it is when HEAD still moves after the last retry or the commit cannot
    /// be staged again. The files are written by then and stay uncommitted.
    /// `between` runs with the attempt number, from 0, after staging and
    /// before HEAD moves. `None` when the new HEAD already holds every write.
    pub fn finish_commit_retrying(
        &self,
        lock: &Lock,
        staged: Staged,
        seen: Option<ObjectId>,
        writes: &[(&str, Option<&[u8]>)],
        message: &str,
        between: &dyn Fn(usize),
    ) -> Result<Option<ObjectId>, WriteError> {
        let mut staged = staged;
        let mut attempt = 0;
        loop {
            if staged.parent != seen {
                for (path, _) in writes {
                    if self.blob_at(seen, path)? != self.blob_at(staged.parent, path)? {
                        return Err(WriteError::HeadMoved);
                    }
                }
            }
            between(attempt);
            match self.finish_commit(lock, staged) {
                Err(WriteError::HeadMoved) if attempt < HEAD_RETRIES => attempt += 1,
                done => return done.map(Some),
            }
            staged = match self.stage_commit(lock, writes, message) {
                Ok(Some(again)) => again,
                Ok(None) => return Ok(None),
                Err(_) => return Err(WriteError::HeadMoved),
            };
        }
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
