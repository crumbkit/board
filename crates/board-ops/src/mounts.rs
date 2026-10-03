//! The boards reachable through mounts (docs/architecture.md, Pointers:
//! links and mounts; The index). board-tree walks them; this indexes each
//! mounted commit once however many places mount it.

use std::collections::{HashMap, HashSet};

use board_index::{IndexError, Writer};
use board_model::{Kind, ObjectId, Source};
use board_tree::{Cache, Entry, Reached, Repository, TreeRow, TreeRows};

use crate::board::Board;
use crate::index::{Indexed, Refresh, RefreshError, Refreshed, facts};

/// Every mounted source, and what the index holds about its entries.
#[derive(Debug)]
pub(crate) struct Mounts {
    pub(crate) reached: Reached,
    facts: HashMap<ObjectId, HashMap<String, Indexed>>,
}

/// The index as the cache a walk through mounts reads and fills.
struct Indexing<'w, 't> {
    writer: &'w Writer<'t>,
    root: &'w HashMap<String, Indexed>,
    facts: HashMap<ObjectId, HashMap<String, Indexed>>,
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
        let mut indexing = Indexing {
            writer,
            root: &refresh.facts,
            facts: HashMap::new(),
            work,
        };
        let reached = self
            .repository()
            .walk_mounts(&refresh.entries, &mut indexing)?;
        Ok(Mounts {
            reached,
            facts: indexing.facts,
        })
    }
}

impl Cache for Indexing<'_, '_> {
    type Error = RefreshError;

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

    /// Adds a mounted commit's blobs' facts first.
    fn pointers(
        &mut self,
        repository: &Repository,
        source: &Source,
        entries: &[Entry],
    ) -> Result<HashSet<String>, RefreshError> {
        let facts = match source {
            Source::WorkingTree => self.root,
            Source::Commit(commit) => {
                let found = facts(self.writer, repository, source, entries, self.work)?;
                self.facts.entry(*commit).or_insert(found)
            }
        };
        Ok(facts
            .iter()
            .filter(|(_, indexed)| {
                indexed.fact.kind == Kind::Pointer && indexed.fact.code.is_none()
            })
            .map(|(path, _)| path.clone())
            .collect())
    }
}

impl Mounts {
    /// What the index holds about the entries of the mounted `commit`.
    pub(crate) fn facts(&self, commit: ObjectId) -> &HashMap<String, Indexed> {
        self.facts.get(&commit).expect(
            "walk_mounts asks for the pointers of every source it reaches, which adds its facts",
        )
    }

    /// Stores every source, the root's repository first, and every mount.
    pub(crate) fn record(&self, writer: &Writer<'_>, root: &Repository) -> Result<(), IndexError> {
        let mut sources = vec![(
            Source::WorkingTree,
            root.head_tree().ok().flatten(),
            root.path(),
        )];
        sources.extend(self.reached.sources.iter().map(|mounted| {
            (
                Source::Commit(mounted.commit),
                Some(mounted.root),
                mounted.repository.path(),
            )
        }));
        writer.set_mounts(&sources, &self.reached.points)
    }
}

#[cfg(test)]
mod tests;
