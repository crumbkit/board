//! One run's decision about every entry (docs/architecture/index.md). A
//! snapshot refreshes the index, walks the boards reachable through mounts,
//! indexing each mounted commit once however many places mount it, decides
//! every entry and commits. What depends only on content comes from the
//! index; whether a reference or pointer resolves depends on the whole tree,
//! so each run decides it again. The index's `entries` and `records` views
//! show what a snapshot decided, so they are read only through one. This
//! snapshot is a decided run over the index; `Repositories::snapshot` is a
//! different thing, a view of the git working tree for resolving paths.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;

use board_index::{Decided, Index, IndexError, Storage, View, Writer};
use board_model::{Change, Kind, LogicalPath, ObjectId, Source, Stat};
use board_tree::{Cache, Entry, Place, Repository, TreeRow, TreeRows, in_scope};

use crate::board::Board;
use crate::check::{CheckError, CheckReport};
use crate::index::{Indexed, Kept, RefreshError, Refreshed, Run, attempt, facts, plain, transact};

mod decide;

use decide::{Decide, Decisions};
pub(crate) use decide::{Referenced, read_type};

/// Every file each source's walk listed, sorted by path: the working tree
/// first, then each mounted commit.
pub(crate) type Walked = Vec<(Source, Vec<Entry>)>;

/// What one run decided about every entry it checked, and the index it
/// committed, whose views can show exactly that.
#[derive(Debug)]
pub(crate) struct Snapshot {
    pub(crate) decided: Decided,
    pub(crate) report: CheckReport,
    /// Every reference each checked record makes, and where it led.
    pub(crate) references: Vec<Referenced>,
    pub(crate) walked: Walked,
}

impl Board {
    /// Decides every entry of the working tree and of each board it mounts,
    /// or only those at or beneath `scope`, after refreshing `index`. An
    /// entry's status is the first problem found: its path, then parsing,
    /// then its schema, validation and references, or for a pointer its
    /// target.
    pub(crate) fn snapshot(
        &self,
        index: Index,
        scope: Option<&LogicalPath>,
    ) -> Result<Snapshot, CheckError> {
        let _timed = self.timed("snapshot");
        let opened = self.repositories().opened();
        if let Some(scope) = scope {
            let _timed = self.timed("resolve");
            self.repositories()
                .resolve(&Source::WorkingTree, scope)
                .map_err(|source| CheckError::Scope { source })?;
        }
        let ((mut report, references, walked), wrote, writer, entries) =
            attempt(index, |writer| self.decide(writer, scope))?;
        let timed = self.timed("snapshot.commit");
        let decided = writer.decide(entries)?;
        drop(timed);
        report.work.wrote = wrote;
        report.work.repositories = self.repositories().opened() - opened;
        report.memory = match decided.storage() {
            Storage::Memory(fallback) => Some(*fallback),
            Storage::File(_) => None,
        };
        let work = &report.work;
        let mut trace = self.trace.borrow_mut();
        trace.count("entries", report.entries.len());
        trace.count("hashed", work.hashed);
        trace.count("parsed", work.parsed);
        trace.count("validated", work.validated);
        trace.count("trees", work.trees);
        trace.count("repositories", work.repositories);
        trace.count("pointers_read", work.pointers_read);
        drop(trace);
        Ok(Snapshot {
            decided,
            report,
            references,
            walked,
        })
    }

    /// Fills the index's views with what a snapshot decided, for `query`,
    /// the one run that reads them.
    pub(crate) fn view(&self, decided: Decided) -> Result<View, IndexError> {
        let _timed = self.timed("view");
        decided.view()
    }

    /// Each mount in the working tree whose repository holds `directory`
    /// ([`board_tree::Repositories::places`]), its pointers read from the index after a
    /// refresh, which is all this decides.
    pub fn places(
        &self,
        directory: &Path,
        cache: Option<&Path>,
    ) -> Result<Vec<Place>, RefreshError> {
        let index = self.index(cache)?;
        let (places, _, _) = transact(index, |writer| {
            let refresh = self.refresh(writer)?;
            let is_pointer = |path: &str| Indexing::is_pointer(&refresh.facts, path);
            let places = (self.repositories()).places(&refresh.entries, &is_pointer, directory);
            Ok((places, Vec::new()))
        })?;
        Ok(places)
    }

    /// Decides each entry, and returns the report, the references, the
    /// files each source's walk listed and the rows of `resolved`.
    fn decide(
        &self,
        writer: &Writer,
        scope: Option<&LogicalPath>,
    ) -> Run<(CheckReport, Vec<Referenced>, Walked)> {
        let refresh = self.refresh(writer)?;
        let mut work = refresh.work;
        let timed = self.timed("snapshot.mounts");
        let mounts = self.repositories().walk_mounts(
            &refresh.entries,
            &refresh.facts,
            &mut Indexing {
                writer,
                work: &mut work,
            },
        )?;
        drop(timed);
        let _timed = self.timed("snapshot.decide");
        let scopes = mounts.scopes(scope);
        let plain =
            plain(std::iter::once(&refresh.facts).chain(mounts.sources.iter().map(|m| &m.facts)));
        let mut known = known(&refresh.entries, &mut work.hashed);
        let resolver = self.repositories().resolver(&mut known, refresh.listings)?;
        let mut resolver = resolver.knowing(&plain);
        let mut decided = Decisions::default();
        let root = Decide {
            repositories: self.repositories(),
            mounted: None,
            writer,
            listed: &refresh.entries,
            facts: &refresh.facts,
            resolve: RefCell::new(Box::new(|path: &LogicalPath| resolver.resolve(path))),
            reader: RefCell::new(self.repositories().root().reader()),
        };
        for entry in &refresh.entries {
            if !in_scope(&scopes, &Source::WorkingTree, entry.logical.as_ref()) {
                continue;
            }
            // A symlink is committed when its own blob is.
            let own = entry.link.as_ref().map(|link| link.blob).or(entry.blob);
            let change = if refresh.head.get(&entry.path) == Some(&own) {
                Change::Committed
            } else {
                Change::Uncommitted
            };
            decided.push(
                &root,
                &Source::WorkingTree,
                entry,
                entry.logical.clone(),
                change,
            )?;
        }
        drop(root);
        work.pointers_read += resolver.pointers_read();
        drop(resolver);
        drop(known);
        for mounted in &mounts.sources {
            let source = Source::Commit(mounted.commit);
            let in_commit = (self.repositories()).in_commit(&mounted.repository, mounted.commit)?;
            let mut in_commit = in_commit.knowing(&plain);
            let decide = Decide {
                repositories: self.repositories(),
                mounted: Some(mounted),
                writer,
                listed: &mounted.entries,
                facts: &mounted.facts,
                resolve: RefCell::new(Box::new(|path: &LogicalPath| in_commit.resolve(path))),
                reader: RefCell::new(self.repositories().root().reader()),
            };
            for entry in &mounted.entries {
                if !in_scope(&scopes, &source, entry.logical.as_ref()) {
                    continue;
                }
                let logical = entry
                    .logical
                    .as_ref()
                    .and_then(|logical| mounted.display(logical));
                decided.push(&decide, &source, entry, logical, Change::Committed)?;
            }
            drop(decide);
            work.pointers_read += in_commit.pointers_read();
        }
        record(&mounts, writer, self.repository())?;
        // Every source is now stored, so a prune keeps what this run reached.
        if writer.wrote()? {
            work.pruned = writer.prune_if_doubled()?;
        }
        let report = CheckReport {
            entries: decided.checked,
            memory: None,
            work,
        };
        let mut walked = vec![(Source::WorkingTree, refresh.entries)];
        walked.extend(
            (mounts.sources.into_iter())
                .map(|mounted| (Source::Commit(mounted.commit), mounted.entries)),
        );
        Ok(((report, decided.references, walked), decided.resolved))
    }
}

/// The blob of each file `entries` saw as it is now, so it need not be
/// hashed again; each file it did not see as it is now counts in `hashed`.
fn known<'e>(
    entries: &'e [Entry],
    hashed: &'e mut usize,
) -> impl FnMut(&str, &Stat) -> Option<ObjectId> + 'e {
    let stats: HashMap<&str, (Stat, Option<ObjectId>)> = entries
        .iter()
        .filter_map(|entry| Some((entry.path.as_str(), (entry.stat?, entry.blob))))
        .collect();
    move |path, stat| {
        let seen = stats.get(path).filter(|(seen, _)| seen == stat);
        *hashed += usize::from(seen.is_none());
        seen.and_then(|(_, blob)| *blob)
    }
}

/// What the index holds about one source's entries, by physical path.
type Facts = HashMap<String, Indexed>;

/// Every mounted source, with what the index holds about its entries.
type Mounts = board_tree::Reached<Facts>;

/// The index as the cache a walk through mounts reads and fills.
struct Indexing<'w> {
    writer: &'w Writer,
    work: &'w mut Refreshed,
}

// A cache read that fails counts as a miss: the index is disposable.
impl Cache for Indexing<'_> {
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
        let kept = Kept::default();
        facts(self.writer, repository, source, entries, self.work, kept)
    }

    fn is_pointer(facts: &Facts, path: &str) -> bool {
        facts.get(path).is_some_and(|indexed| {
            indexed.fact.kind == Kind::Pointer && indexed.fact.code.is_none()
        })
    }
}

/// Stores every source, the root's repository first, and every mount.
fn record(mounts: &Mounts, writer: &Writer, root: &Repository) -> Result<(), IndexError> {
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
