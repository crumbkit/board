//! The boards reachable through mounts (docs/architecture/paths.md, Pointers:
//! links and mounts; docs/architecture/index.md). board-tree walks them; this indexes each
//! mounted commit once however many places mount it.

use std::collections::HashMap;

use board_index::{IndexError, Writer};
use board_model::{Kind, ObjectId, Source};
use board_tree::{Cache, Entry, Reached, Repository, TreeRow, TreeRows};

use crate::board::Board;
use crate::index::{Indexed, Refresh, RefreshError, Refreshed, facts};

/// What the index holds about one source's entries, by physical path.
type Facts = HashMap<String, Indexed>;

/// Every mounted source, with what the index holds about its entries.
pub(crate) type Mounts = Reached<Facts>;

/// The index as the cache a walk through mounts reads and fills.
struct Indexing<'w, 't> {
    writer: &'w Writer<'t>,
    work: &'w mut Refreshed,
}

impl Board {
    /// Finds every board reachable through mounts from the root's `refresh`,
    /// indexing each mounted commit the index does not hold.
    pub(crate) fn mounts(
        &self,
        writer: &Writer<'_>,
        refresh: &Refresh,
        work: &mut Refreshed,
    ) -> Result<Mounts, RefreshError> {
        self.repository().walk_mounts(
            &refresh.entries,
            &refresh.facts,
            &mut Indexing { writer, work },
        )
    }
}

// A cache read that fails counts as a miss: the index is disposable.
impl Cache for Indexing<'_, '_> {
    type Error = RefreshError;
    type Facts = Facts;

    fn has_tree(&mut self, tree: ObjectId) -> bool {
        self.writer.has_tree(tree).unwrap_or(false)
    }

    fn put_tree(&mut self, tree: &TreeRows) -> Result<(), RefreshError> {
        self.writer.put_tree(tree)?;
        self.work.trees += 1;
        Ok(())
    }

    fn rows(&mut self, tree: ObjectId) -> Option<Vec<TreeRow>> {
        self.writer.tree(tree).ok().flatten()
    }

    fn facts(
        &mut self,
        repository: &Repository,
        source: &Source,
        entries: &[Entry],
    ) -> Result<Facts, RefreshError> {
        facts(self.writer, repository, source, entries, self.work)
    }

    fn is_pointer(facts: &Facts, path: &str) -> bool {
        facts.get(path).is_some_and(|indexed| {
            indexed.fact.kind == Kind::Pointer && indexed.fact.code.is_none()
        })
    }
}

/// Stores every source, the root's repository first, and every mount.
pub(crate) fn record(
    mounts: &Mounts,
    writer: &Writer<'_>,
    root: &Repository,
) -> Result<(), IndexError> {
    let mut sources = vec![(
        Source::WorkingTree,
        root.head_tree().ok().flatten(),
        root.path(),
    )];
    sources.extend(mounts.sources.iter().map(|mounted| {
        (
            Source::Commit(mounted.commit),
            Some(mounted.root),
            mounted.repository.path(),
        )
    }));
    writer.set_mounts(&sources, &mounts.points)
}

#[cfg(test)]
mod tests;
