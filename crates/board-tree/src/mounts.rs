//! The boards reachable through mounts (docs/architecture.md, Pointers:
//! links and mounts; The index). Each mounted commit is one source, walked
//! once however many places mount it.

use std::collections::{HashMap, HashSet};

use board_model::{Code, LogicalPath, Mount, Object, ObjectId, Pointer, Source};

use crate::git::{GitError, Repository};
use crate::trees::{TreeRow, TreeRows, walk_stored};
use crate::walk::{Entry, Status};
use crate::write::{Target, WriteError};

/// What a cache, such as the index, holds about trees and blobs, so a walk
/// through mounts reads only what it does not hold.
pub trait Cache {
    type Error: From<GitError>;
    /// What the cache holds about one source's entries.
    type Facts;
    fn has_tree(&mut self, tree: ObjectId) -> bool;
    /// Stores the rows of a tree the cache did not hold.
    fn put_tree(&mut self, tree: &TreeRows) -> Result<(), Self::Error>;
    /// The rows of `tree`, or `None` when they are not held.
    fn rows(&mut self, tree: ObjectId) -> Option<Vec<TreeRow>>;
    /// The facts about the `entries` of a mounted `source`.
    fn facts(
        &mut self,
        repository: &Repository,
        source: &Source,
        entries: &[Entry],
    ) -> Result<Self::Facts, Self::Error>;
    /// Whether the entry at the physical `path` holds a pointer that parses.
    fn is_pointer(facts: &Self::Facts, path: &str) -> bool;
}

/// A mounted commit, its entries and the cache's facts about them.
#[derive(Debug)]
pub struct Mounted<F> {
    pub commit: ObjectId,
    pub root: ObjectId,
    /// The repository it was first read from. A relative `repo` in one of
    /// its own mounts is read against this repository.
    pub repository: Repository,
    /// The display path of its first route, by shortest display path and
    /// then lexically; its entries are named beneath it.
    pub route: String,
    pub entries: Vec<Entry>,
    pub facts: F,
}

impl<F> Mounted<F> {
    /// The display path of `logical`, a path inside this source.
    pub fn display(&self, logical: &LogicalPath) -> Option<LogicalPath> {
        join(&self.route, logical).parse().ok()
    }
}

/// A mount pointer and the commit it reaches, or the code that says why it
/// cannot be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MountPoint {
    pub source: Source,
    /// The pointer's physical path in its source.
    pub path: String,
    pub target: Result<ObjectId, Code>,
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
#[derive(Debug)]
pub struct Reached<F> {
    pub sources: Vec<Mounted<F>>,
    pub points: Vec<MountPoint>,
    edges: Vec<Edge>,
}

/// A mount found but not yet walked.
struct Candidate {
    route: String,
    commit: ObjectId,
    root: ObjectId,
    repository: Repository,
}

impl Repository {
    /// Every board reachable through mounts from this working tree, whose
    /// entries are `entries` with the cache's `facts` about them. Each
    /// mounted commit's trees that `cache` does not hold are stored in it,
    /// then walked from it.
    pub fn walk_mounts<C: Cache>(
        &self,
        entries: &[Entry],
        facts: &C::Facts,
        cache: &mut C,
    ) -> Result<Reached<C::Facts>, C::Error> {
        let mut reached = Reached {
            sources: Vec::new(),
            points: Vec::new(),
            edges: Vec::new(),
        };
        let mut candidates = Vec::new();
        let found = pointers::<C>(self, &Source::WorkingTree, entries, facts);
        reached.add(self, &Source::WorkingTree, "", found, &mut candidates);
        let mut seen: HashSet<ObjectId> = HashSet::new();
        while let Some(at) = first(&candidates) {
            let candidate = candidates.swap_remove(at);
            if !seen.insert(candidate.commit) {
                continue;
            }
            let mounted = walk(candidate, cache)?;
            let source = Source::Commit(mounted.commit);
            let found = pointers::<C>(
                &mounted.repository,
                &source,
                &mounted.entries,
                &mounted.facts,
            );
            reached.add(
                &mounted.repository,
                &source,
                &mounted.route,
                found,
                &mut candidates,
            );
            reached.sources.push(mounted);
        }
        Ok(reached)
    }

    /// The mount pointer at `path`, which is not followed, and the commit
    /// its `follow` ref names now.
    pub fn pin(&self, path: &LogicalPath) -> Result<Pin, PinError> {
        let not_a_mount = || PinError::NotAMount { path: path.clone() };
        let refused = |code| PinError::Mount {
            path: path.clone(),
            code,
        };
        let target = self.pointer_target(path)?;
        let Object::Blob(blob) = target.location.object else {
            return Err(not_a_mount());
        };
        let bytes = self
            .read_working_file(&target.path)?
            .ok_or_else(not_a_mount)?;
        let mount = match board_model::pointer(&bytes) {
            Ok(Some(Pointer::Mount(mount))) => mount,
            Ok(_) => return Err(not_a_mount()),
            Err(error) => return Err(refused(error.code())),
        };
        let new = self.followed(&mount).map_err(refused)?;
        let canonical =
            board_model::repinned(&bytes, new).map_err(|error| refused(error.code()))?;
        Ok(Pin {
            target,
            blob,
            old: mount.commit,
            new,
            canonical,
        })
    }
}

/// A mount pointer in the working tree, ready to move to its `follow`.
#[derive(Debug)]
pub struct Pin {
    pub target: Target,
    /// The pointer's blob now.
    pub blob: ObjectId,
    pub old: ObjectId,
    pub new: ObjectId,
    /// The pointer with its commit set to `new`, in canonical form.
    pub canonical: Vec<u8>,
}

/// A mount pointer that cannot be pinned to its `follow`.
#[derive(Debug, thiserror::Error)]
pub enum PinError {
    #[error("{path} is not a mount pointer")]
    NotAMount { path: LogicalPath },
    #[error("the mount at {path} cannot be updated: {code}")]
    Mount { path: LogicalPath, code: Code },
    #[error(transparent)]
    Write(#[from] WriteError),
}

impl PinError {
    pub fn code(&self) -> Code {
        match self {
            Self::NotAMount { .. } => Code::MountInvalid,
            Self::Mount { code, .. } => *code,
            Self::Write(error) => error.code(),
        }
    }
}

impl<F> Reached<F> {
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
            self.points.push(MountPoint {
                source: source.clone(),
                path,
                target: mounted.as_ref().map(|_| mount.commit).map_err(|code| *code),
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
    pub fn scopes(&self, scope: Option<&LogicalPath>) -> Scopes {
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
pub type Scopes = HashMap<Source, Vec<Option<Vec<String>>>>;

/// Whether the entry at `logical` inside `source` is in scope on any route.
pub fn in_scope(scopes: &Scopes, source: &Source, logical: Option<&LogicalPath>) -> bool {
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

/// Stores a mounted commit's trees that `cache` does not hold, walks it from
/// `cache`, and gets the cache's facts about its entries.
fn walk<C: Cache>(candidate: Candidate, cache: &mut C) -> Result<Mounted<C::Facts>, C::Error> {
    let Candidate {
        route,
        commit,
        root,
        repository,
    } = candidate;
    for tree in repository.trees(root, &mut |tree| cache.has_tree(tree))? {
        cache.put_tree(&tree)?;
    }
    let entries = walk_stored(repository.path(), commit, root, &mut |tree| {
        cache.rows(tree)
    })?;
    let facts = cache.facts(&repository, &Source::Commit(commit), &entries)?;
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
fn pointers<C: Cache>(
    repository: &Repository,
    source: &Source,
    entries: &[Entry],
    facts: &C::Facts,
) -> Vec<(LogicalPath, String, Mount)> {
    entries
        .iter()
        .filter(|entry| entry.status == Status::Ok && C::is_pointer(facts, &entry.path))
        .filter_map(|entry| {
            let bytes = repository.read(source, &entry.path, entry.blob?).ok()??;
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
fn join(route: &str, logical: &LogicalPath) -> String {
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
