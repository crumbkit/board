//! The boards reachable through mounts (docs/architecture.md, Pointers:
//! links and mounts; The index). Each mounted commit is one source, indexed
//! once however many places mount it.

use std::collections::{HashMap, HashSet};

use board_index::{IndexError, MountRow, Writer};
use board_model::{Kind, LogicalPath, Mount, ObjectId, Pointer, Source};
use board_tree::{Entry, Repository, Status, walk_stored};

use crate::board::Board;
use crate::index::{Indexed, Refresh, RefreshError, Refreshed, facts};

/// A mounted commit and what the index holds about its entries.
#[derive(Debug)]
pub(crate) struct Mounted {
    pub(crate) commit: ObjectId,
    pub(crate) root: ObjectId,
    /// The repository it was first read from. A relative `repo` in one of
    /// its own mounts is read against this repository.
    pub(crate) repository: Repository,
    /// The display path of its first route, by shortest display path and
    /// then lexically; its entries are named beneath it.
    pub(crate) route: String,
    pub(crate) entries: Vec<Entry>,
    pub(crate) facts: HashMap<String, Indexed>,
}

/// A pointer in one source that mounts a commit.
#[derive(Debug)]
struct Edge {
    from: Source,
    /// The pointer's logical path inside its own source.
    at: LogicalPath,
    to: ObjectId,
}

/// Every mounted source, and how they are mounted.
#[derive(Debug, Default)]
pub(crate) struct Mounts {
    pub(crate) sources: Vec<Mounted>,
    pub(crate) rows: Vec<MountRow>,
    edges: Vec<Edge>,
}

/// A mount found but not yet indexed.
struct Candidate {
    route: String,
    commit: ObjectId,
    root: ObjectId,
    repository: Repository,
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
        let mut mounts = Mounts::default();
        let mut candidates = Vec::new();
        let root = self.repository();
        let found = pointers(root, &Source::WorkingTree, &refresh.entries, &refresh.facts);
        mounts.add(root, &Source::WorkingTree, "", found, &mut candidates);
        let mut seen: HashSet<ObjectId> = HashSet::new();
        while let Some(at) = first(&candidates) {
            let candidate = candidates.swap_remove(at);
            if !seen.insert(candidate.commit) {
                continue;
            }
            let mounted = index(writer, candidate, work)?;
            let source = Source::Commit(mounted.commit);
            let found = pointers(
                &mounted.repository,
                &source,
                &mounted.entries,
                &mounted.facts,
            );
            mounts.add(
                &mounted.repository,
                &source,
                &mounted.route,
                found,
                &mut candidates,
            );
            mounts.sources.push(mounted);
        }
        Ok(mounts)
    }
}

impl Mounts {
    /// Stores every source, the root's repository first, and every mount.
    pub(crate) fn record(&self, writer: &Writer<'_>, root: &Repository) -> Result<(), IndexError> {
        let mut sources = vec![(
            Source::WorkingTree,
            root.head_tree().ok().flatten(),
            root.path(),
        )];
        sources.extend(self.sources.iter().map(|mounted| {
            (
                Source::Commit(mounted.commit),
                Some(mounted.root),
                mounted.repository.path(),
            )
        }));
        writer.set_mounts(&sources, &self.rows)
    }

    /// Records the mounts `found` in `source`, whose route is `route`, and
    /// adds each one that can be read as a candidate.
    fn add(
        &mut self,
        repository: &Repository,
        source: &Source,
        route: &str,
        found: Vec<(LogicalPath, String, Mount)>,
        candidates: &mut Vec<Candidate>,
    ) {
        for (at, path, mount) in found {
            let mounted = repository.mounted(&mount);
            self.rows.push(MountRow {
                source: source.clone(),
                path,
                target: mounted.is_ok().then_some(mount.commit),
                code: mounted.as_ref().err().copied(),
            });
            let Ok((mounted, root)) = mounted else {
                continue;
            };
            candidates.push(Candidate {
                route: join(route, &at),
                commit: mount.commit,
                root,
                repository: mounted,
            });
            self.edges.push(Edge {
                from: source.clone(),
                at,
                to: mount.commit,
            });
        }
    }

    /// For each source, what remains of `scope` at its root on each route
    /// that reaches it: `None` when the whole source is in scope. Without a
    /// scope, every source is wholly in it.
    pub(crate) fn scopes(&self, scope: Option<&LogicalPath>) -> Scopes {
        let mut scopes: Scopes = HashMap::new();
        let start = scope.map(|scope| scope.segments().map(str::to_owned).collect());
        let mut pending = vec![(Source::WorkingTree, start)];
        while let Some((source, remaining)) = pending.pop() {
            let states = scopes.entry(source.clone()).or_default();
            if states.contains(&remaining) {
                continue;
            }
            states.push(remaining.clone());
            for edge in self.edges.iter().filter(|edge| edge.from == source) {
                let at: Vec<&str> = edge.at.segments().collect();
                let next = match &remaining {
                    None => None,
                    Some(remaining) if remaining.len() <= at.len() => {
                        if remaining.iter().zip(&at).all(|(a, b)| a == b) {
                            None
                        } else {
                            continue;
                        }
                    }
                    Some(remaining) => {
                        if remaining.iter().zip(&at).all(|(a, b)| a == b) {
                            Some(remaining[at.len()..].to_vec())
                        } else {
                            continue;
                        }
                    }
                };
                pending.push((Source::Commit(edge.to), next));
            }
        }
        scopes
    }
}

/// What remains of a scope at each source's root, on each route to it.
pub(crate) type Scopes = HashMap<Source, Vec<Option<Vec<String>>>>;

/// Whether the entry at `logical` inside `source` is in scope on any route.
pub(crate) fn in_scope(scopes: &Scopes, source: &Source, logical: Option<&LogicalPath>) -> bool {
    let Some(states) = scopes.get(source) else {
        return false;
    };
    states.iter().any(|remaining| match remaining {
        None => true,
        Some(remaining) => logical.is_some_and(|logical| {
            let mut segments = logical.segments();
            remaining
                .iter()
                .all(|segment| segments.next() == Some(segment.as_str()))
        }),
    })
}

/// The candidate with the shortest route, then the first lexically.
fn first(candidates: &[Candidate]) -> Option<usize> {
    candidates
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| (a.route.len(), &a.route).cmp(&(b.route.len(), &b.route)))
        .map(|(at, _)| at)
}

/// Adds a mounted commit's trees the index does not hold, then its blobs'
/// facts, and walks it from the index.
fn index(
    writer: &Writer<'_>,
    candidate: Candidate,
    work: &mut Refreshed,
) -> Result<Mounted, RefreshError> {
    let Candidate {
        route,
        commit,
        root,
        repository,
    } = candidate;
    let mut known = |tree| writer.has_tree(tree).unwrap_or(false);
    for tree in repository.trees(root, &mut known)? {
        writer.put_tree(&tree)?;
        work.trees += 1;
    }
    let mut rows = |tree| writer.tree(tree).ok().flatten();
    let entries = walk_stored(repository.path(), commit, root, &mut rows)?;
    let facts = facts(writer, &repository, &Source::Commit(commit), &entries, work)?;
    Ok(Mounted {
        commit,
        root,
        repository,
        route,
        entries,
        facts,
    })
}

/// Every mount among `entries` of `source`: its logical and physical paths
/// in the source, and what it mounts.
fn pointers(
    repository: &Repository,
    source: &Source,
    entries: &[Entry],
    facts: &HashMap<String, Indexed>,
) -> Vec<(LogicalPath, String, Mount)> {
    entries
        .iter()
        .filter(|entry| entry.status == Status::Ok)
        .filter_map(|entry| {
            let fact = facts.get(&entry.path)?;
            if fact.fact.kind != Kind::Pointer || fact.fact.code.is_some() {
                return None;
            }
            let bytes = repository.read(source, &entry.path, fact.blob).ok()??;
            match board_model::pointer(&bytes) {
                Ok(Some(Pointer::Mount(mount))) => {
                    Some((entry.logical.clone()?, entry.path.clone(), mount))
                }
                _ => None,
            }
        })
        .collect()
}

/// `logical`, a path inside a source, under that source's `route`.
pub(crate) fn join(route: &str, logical: &LogicalPath) -> String {
    let inside = logical.to_string();
    if inside == "/" {
        if route.is_empty() {
            "/".to_owned()
        } else {
            route.to_owned()
        }
    } else {
        format!("{route}{inside}")
    }
}

#[cfg(test)]
mod tests;
