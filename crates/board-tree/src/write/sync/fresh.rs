//! A sync that runs no fetch when the clone was fetched a moment ago
//! (docs/architecture/writes/sync/fresh.md).

use std::time::{Duration, SystemTime};

use super::{SyncError, Synced};
use crate::git::Repository;
use crate::transport::Deadline;

impl Repository {
    /// As [`Self::sync`], but a sync without `push` runs no fetch when the
    /// clone was fetched less than `within` ago and HEAD already holds what
    /// that fetch brought. It then changes nothing, takes no lock, and
    /// returns HEAD as both `old` and `new`, the remote-tracking commit as
    /// `fetched`, and zero `attempts`. The last fetch is fresh when git's
    /// `FETCH_HEAD` is not empty and was written less than `within` ago.
    /// git rewrites `FETCH_HEAD` only after it has reached the remote, so a
    /// fetch that cannot reach it leaves the previous fetch's file and date,
    /// and one that fails after that leaves it empty, which never counts.
    /// A clone mid-merge or lacking history syncs in full, so it is refused
    /// as [`Self::sync`] refuses it. Any other clone, and every sync with
    /// `push`, syncs in full (docs/architecture/writes/sync/fresh.md).
    pub fn sync_unless_fetched(
        &self,
        push: bool,
        within: Duration,
        deadline: Deadline,
        wait: Duration,
    ) -> Result<Synced, SyncError> {
        if !push && let Some(synced) = self.fetched_within(within)? {
            return Ok(synced);
        }
        self.sync(push, deadline, wait)
    }

    /// HEAD as a sync that fetched nothing reports it, when the last fetch
    /// is younger than `within`, HEAD holds the remote-tracking commit and
    /// no refusal before integration applies, or `None` when the sync must
    /// run in full.
    fn fetched_within(&self, within: Duration) -> Result<Option<Synced>, SyncError> {
        let upstream = self.upstream()?;
        if self.operation_in_progress().is_some() || self.incomplete(&upstream.remote).is_some() {
            return Ok(None);
        }
        let fresh = std::fs::metadata(self.git_dir().join("FETCH_HEAD"))
            .ok()
            .filter(|metadata| metadata.len() > 0)
            .and_then(|metadata| metadata.modified().ok())
            .and_then(|modified| SystemTime::now().duration_since(modified).ok())
            .is_some_and(|age| age < within);
        if !fresh {
            return Ok(None);
        }
        let (Some(fetched), Some(head)) = (self.tracking_before(&upstream)?, self.head()?) else {
            return Ok(None);
        };
        if head != fetched && !self.contains(head, fetched) {
            return Ok(None);
        }
        Ok(Some(Synced {
            upstream,
            fetched,
            old: Some(head),
            new: head,
            merged: false,
            conflicts: Vec::new(),
            pins: Vec::new(),
            failing: Vec::new(),
            pushed: false,
            attempts: 0,
            kept: Vec::new(),
        }))
    }
}
