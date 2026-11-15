//! The refusals of a sync that come before it changes the working tree, index or HEAD: a git
//! operation in progress (the put's list of them), a shallow or partial
//! clone, and a remote branch that no longer contains the commit the clone
//! fetched last time (docs/architecture/writes.md, Sync).

use board_model::ObjectId;

use super::SyncError;
use super::integrate::common_ancestor;
use crate::git::Repository;
use crate::transport::Upstream;

/// What the clone held before this round's fetch.
#[derive(Clone, Copy)]
pub(super) struct Before {
    /// The remote-tracking commit the fetched one must descend from, or
    /// `None` when the clone had none or the caller accepts a rewrite.
    pub previous: Option<ObjectId>,
    /// The remote-tracking commit the ref returns to when the sync is
    /// refused as rewound, so a later run is refused too.
    pub restore: Option<ObjectId>,
}

impl Repository {
    /// Refuses before the board lock is taken and before any working-tree file,
    /// index entry or HEAD is written. Every refusal puts the
    /// remote-tracking ref back where it was before the fetch, so the next
    /// run compares the remote with the same commit. A ref that cannot be put
    /// back is reported by the refusal.
    pub(super) fn refuse_unsafe(
        &self,
        upstream: &Upstream,
        fetched: ObjectId,
        before: Before,
    ) -> Result<(), SyncError> {
        let Some(refusal) = self.refusal(upstream, fetched, before.previous) else {
            return Ok(());
        };
        let unrestored = !self.put_back(upstream, fetched, before.restore);
        Err(refusal.with_unrestored(unrestored))
    }

    /// The refusal for this clone and fetched commit, if there is one.
    fn refusal(
        &self,
        upstream: &Upstream,
        fetched: ObjectId,
        previous: Option<ObjectId>,
    ) -> Option<SyncError> {
        if let Some(operation) = self.operation_in_progress() {
            return Some(SyncError::Merging {
                operation,
                unrestored: false,
            });
        }
        if let Some(kind) = self.incomplete(&upstream.remote) {
            return Some(SyncError::Shallow {
                kind,
                unrestored: false,
            });
        }
        let previous = previous?;
        if previous == fetched || self.contains(fetched, previous) {
            return None;
        }
        Some(SyncError::Rewound {
            remote: upstream.remote.clone(),
            branch: upstream.branch.clone(),
            previous,
            fetched,
            unrestored: false,
        })
    }

    /// `shallow` or `partial` when the clone lacks history, else `None`.
    fn incomplete(&self, remote: &str) -> Option<&'static str> {
        let git = self.git();
        if git.is_shallow() {
            return Some("shallow");
        }
        let config = git.config_snapshot();
        let promisor = config.string("extensions.partialclone").is_some()
            || config
                .boolean(format!("remote.{remote}.promisor").as_str())
                .unwrap_or(false);
        promisor.then_some("partial")
    }

    /// Whether `commit` has `ancestor` in its history. A history that
    /// cannot be read (a pruned `ancestor`) does not.
    fn contains(&self, commit: ObjectId, ancestor: ObjectId) -> bool {
        let ids = (
            gix::ObjectId::try_from(commit.as_bytes()),
            gix::ObjectId::try_from(ancestor.as_bytes()),
        );
        let (Ok(commit), Ok(ancestor)) = ids else {
            return false;
        };
        matches!(common_ancestor(self.git(), commit, ancestor), Ok(Some(base)) if base == ancestor)
    }

    /// Points the remote-tracking ref back at `commit` from `fetched`.
    /// Returns whether the ref is where it should be: true when there is
    /// nothing to restore, false when the commit is missing from the store or
    /// the ref moved or could not be updated.
    fn put_back(&self, upstream: &Upstream, fetched: ObjectId, commit: Option<ObjectId>) -> bool {
        use gix::refs::transaction::PreviousValue;
        let Some(commit) = commit.filter(|commit| *commit != fetched) else {
            return true;
        };
        let ids = (
            gix::ObjectId::try_from(commit.as_bytes()),
            gix::ObjectId::try_from(fetched.as_bytes()),
        );
        let (Ok(id), Ok(fetched)) = ids else {
            return false;
        };
        if self.git().find_commit(id).is_err() {
            return false;
        }
        self.git()
            .reference(
                upstream.tracking.as_str(),
                id,
                PreviousValue::MustExistAndMatch(gix::refs::Target::Object(fetched)),
                "board sync: refused to sync",
            )
            .is_ok()
    }
}

impl SyncError {
    /// This refusal with its `unrestored` flag set.
    fn with_unrestored(self, flag: bool) -> Self {
        match self {
            Self::Merging { operation, .. } => Self::Merging {
                operation,
                unrestored: flag,
            },
            Self::Shallow { kind, .. } => Self::Shallow {
                kind,
                unrestored: flag,
            },
            Self::Rewound {
                remote,
                branch,
                previous,
                fetched,
                ..
            } => Self::Rewound {
                remote,
                branch,
                previous,
                fetched,
                unrestored: flag,
            },
            other => other,
        }
    }
}

#[cfg(test)]
mod tests;
