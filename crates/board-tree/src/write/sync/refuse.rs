//! The refusals of a sync that come before it writes anything: a git merge
//! or rebase in progress, a shallow or partial clone, and a remote branch
//! that no longer contains the commit the clone fetched last time
//! (docs/architecture/writes.md, Sync).

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

/// The files git keeps in its directory while an operation is in progress,
/// and its name.
const OPERATIONS: [(&str, &str); 3] = [
    ("MERGE_HEAD", "merge"),
    ("rebase-merge", "rebase"),
    ("rebase-apply", "rebase"),
];

impl Repository {
    /// Refuses before the board lock is taken and before any file, object,
    /// index entry or HEAD is written. A refusal as rewound puts the
    /// remote-tracking ref back where it was before the fetch.
    pub(super) fn refuse_unsafe(
        &self,
        upstream: &Upstream,
        fetched: ObjectId,
        before: Before,
    ) -> Result<(), SyncError> {
        for (file, operation) in OPERATIONS {
            if self.git_dir().join(file).exists() {
                return Err(SyncError::Merging { operation });
            }
        }
        if let Some(kind) = self.incomplete(&upstream.remote) {
            return Err(SyncError::Shallow { kind });
        }
        let Some(previous) = before.previous else {
            return Ok(());
        };
        if previous == fetched || self.contains(fetched, previous) {
            return Ok(());
        }
        self.put_back(upstream, before.restore);
        Err(SyncError::Rewound {
            remote: upstream.remote.clone(),
            branch: upstream.branch.clone(),
            previous,
            fetched,
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

    /// Points the remote-tracking ref back at `commit`, when the store holds
    /// it. A ref that cannot be moved is left, and the refusal stands.
    fn put_back(&self, upstream: &Upstream, commit: Option<ObjectId>) {
        use gix::refs::transaction::PreviousValue;
        let Some(id) = commit.and_then(|id| gix::ObjectId::try_from(id.as_bytes()).ok()) else {
            return;
        };
        if self.git().find_commit(id).is_err() {
            return;
        }
        let _ = self.git().reference(
            upstream.tracking.as_str(),
            id,
            PreviousValue::Any,
            "board sync: refused a rewound remote",
        );
    }
}
