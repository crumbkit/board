//! A guarded sync: `board sync` that refuses where an unattended sync must
//! not guess. It refuses a HEAD on another branch, an index that differs
//! from HEAD and a git operation in progress before it fetches, refuses a
//! merge with any conflict before HEAD moves, and asks before each push
//! (docs/architecture/writes.md, Sync).

use std::collections::HashMap;

use board_model::ObjectId;
use gix::bstr::BStr;
use gix::index::entry::Stage;

use super::SyncError;
use crate::git::Repository;

/// What a guarded sync checks beyond a plain one. Its push is asked of
/// `before_push` for each push it would make, with the commits it would
/// send.
pub struct Guard<'a> {
    /// The branch HEAD must be on, such as `main`.
    pub branch: String,
    /// Asked before every push, including one that starts the remote
    /// branch. [`BeforePush::Hold`] stops the sync with
    /// [`SyncError::Held`], with HEAD where the merge left it.
    pub before_push: &'a mut dyn FnMut(&Pushing) -> BeforePush,
}

impl std::fmt::Debug for Guard<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        (formatter.debug_struct("Guard"))
            .field("branch", &self.branch)
            .finish_non_exhaustive()
    }
}

/// The commits a push would send: those reachable from `new` and not from
/// `fetched`, the remote branch after this round's fetch, or every commit
/// reachable from `new` when the remote has no branch yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pushing {
    pub fetched: Option<ObjectId>,
    /// The commit the push names by id, so a put while the guard runs adds
    /// nothing to what it sends.
    pub new: ObjectId,
}

/// What a guard answers before a push.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BeforePush {
    Push,
    Hold,
}

/// Whether a merge that conflicts records each conflict and goes on, as
/// `board sync` does, or is refused before HEAD moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum OnConflict {
    Record,
    Refuse,
}

impl Repository {
    /// Refuses before any fetch when HEAD is not on `branch`, a git
    /// operation is in progress, or the index differs from HEAD's tree.
    pub(super) fn refuse_unguarded(&self, branch: &str) -> Result<(), SyncError> {
        let found = (self.git().head_name())
            .map_err(|source| self.unreadable(source))?
            .map(|name| name.shorten().to_string());
        if found.as_deref() != Some(branch) {
            return Err(SyncError::OtherBranch {
                expected: branch.to_owned(),
                found,
            });
        }
        if let Some(operation) = self.operation_in_progress() {
            return Err(SyncError::Merging {
                operation,
                unrestored: false,
            });
        }
        let paths = self.staged()?;
        if !paths.is_empty() {
            return Err(SyncError::Staged { paths });
        }
        Ok(())
    }

    /// Each path where the index differs from HEAD's tree: an entry with
    /// another blob or mode, an entry HEAD lacks, a HEAD file the index
    /// lacks, and every conflicted entry. A clone with no index file, or
    /// with no commit, has staged nothing.
    fn staged(&self) -> Result<Vec<String>, SyncError> {
        let git = self.git();
        if !git.index_path().exists() {
            return Ok(Vec::new());
        }
        let Ok(tree) = git.head_tree_id() else {
            return Ok(Vec::new());
        };
        let committed = (git.index_from_tree(&tree)).map_err(|source| self.unreadable(source))?;
        let index = git.open_index().map_err(|source| self.unreadable(source))?;
        let held: HashMap<&BStr, &gix::index::Entry> = (committed.entries().iter())
            .map(|entry| (entry.path(&committed), entry))
            .collect();
        let mut paths: Vec<String> = Vec::new();
        for entry in index.entries() {
            let path = entry.path(&index);
            let same = held.get(path).is_some_and(|committed| {
                committed.id == entry.id && committed.mode == entry.mode
            });
            if !same || entry.stage() != Stage::Unconflicted {
                paths.push(path.to_string());
            }
        }
        let indexed: std::collections::HashSet<&BStr> =
            index.entries().iter().map(|entry| entry.path(&index)).collect();
        paths.extend(
            (held.keys())
                .filter(|path| !indexed.contains(*path))
                .map(ToString::to_string),
        );
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}

#[cfg(test)]
mod tests;
