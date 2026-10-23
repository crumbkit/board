//! The boards reachable through mounts (docs/architecture/paths.md, Pointers:
//! links and mounts; docs/architecture/index.md). Each mounted commit is one source, walked
//! once however many places mount it.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use board_model::{
    Code, Fields, LogicalPath, Malformation, Mount, Object, ObjectId, PathError, Pointer, Source,
};

use crate::git::{Contents, GitError, Repository, latest};
use crate::repositories::Repositories;
use crate::tree::Tree;
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

impl Repositories {
    /// Every board reachable through mounts from the board's working tree,
    /// whose entries are `entries` with the cache's `facts` about them. Each
    /// mounted commit's trees that `cache` does not hold are stored in it,
    /// then walked from it. A repository that several pointers name is
    /// opened once.
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
        let root = self.root();
        let is_pointer = |path: &str| C::is_pointer(facts, path);
        let found = pointers(root, &Source::WorkingTree, entries, &is_pointer);
        let from = root.path();
        reached.add(self, from, &Source::WorkingTree, "", found, &mut candidates);
        let mut seen: HashSet<ObjectId> = HashSet::new();
        while let Some(at) = first(&candidates) {
            let candidate = candidates.swap_remove(at);
            if !seen.insert(candidate.commit) {
                continue;
            }
            let mounted = walk(candidate, cache)?;
            let source = Source::Commit(mounted.commit);
            let is_pointer = |path: &str| C::is_pointer(&mounted.facts, path);
            let found = pointers(&mounted.repository, &source, &mounted.entries, &is_pointer);
            reached.add(
                self,
                mounted.repository.path(),
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
        let Object::Blob(resolved) = target.location.object else {
            return Err(not_a_mount());
        };
        let root = self.root();
        let Contents { blob, bytes } =
            latest((root.reader()).read(&Source::WorkingTree, &target.path, resolved))
                .map_err(|error| refused(error.code()))?
                .ok_or_else(not_a_mount)?;
        let mount = match board_model::pointer(&bytes) {
            Ok(Some(Pointer::Mount(mount))) => mount,
            Ok(_) => return Err(not_a_mount()),
            Err(error) => return Err(refused(error.code())),
        };
        let new = self.followed(root.path(), &mount).map_err(refused)?;
        let canonical =
            board_model::repinned(&bytes, new).map_err(|error| refused(error.code()))?;
        Ok(Pin {
            target,
            blob,
            old: mount.commit,
            follow: mount.follow,
            new,
            canonical,
        })
    }
}

/// Where a directory is in a board: the mount whose repository holds it,
/// and the directory's path beneath that repository's working tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    pub mount: LogicalPath,
    pub within: LogicalPath,
}

impl Place {
    /// The directory's logical path in the board, such as
    /// `/repos/crumbkit/docs`.
    pub fn prefix(&self) -> String {
        join(self.mount.as_str(), &self.within)
    }
}

impl Repositories {
    /// Each mount among `entries`, the working tree's, whose repository is
    /// the one holding `directory`. Repositories are compared by the git
    /// directory a linked worktree shares with its main one, so a linked
    /// worktree matches too. `is_pointer` says, from a cache, which physical
    /// paths hold a pointer that parses. Each call opens and compares every
    /// mounted repository again.
    pub fn places(
        &self,
        entries: &[Entry],
        is_pointer: &dyn Fn(&str) -> bool,
        directory: &Path,
    ) -> Vec<Place> {
        let Some((common, Ok(within))) = holding(directory) else {
            return Vec::new();
        };
        let root = self.root();
        let shares = |mount: &Mount| {
            let mounted = self.mounted(root.path(), mount);
            mounted.and_then(|mounted| mounted.git().common_dir().canonicalize().ok())
                == Some(common.clone())
        };
        pointers(root, &Source::WorkingTree, entries, is_pointer)
            .into_iter()
            .filter(|(_, _, mount)| shares(mount))
            .map(|(mount, _, _)| Place {
                mount,
                within: within.clone(),
            })
            .collect()
    }
}

/// `directory`'s path beneath the working tree of the repository holding
/// it, as `git rev-parse --show-prefix` gives it, such as `/docs`, or the
/// refusal of that path when it is not a logical path.
pub fn within(directory: &Path) -> Option<Result<LogicalPath, PathError>> {
    holding(directory).map(|(_, within)| within)
}

/// The canonical git directory of the repository whose working tree holds
/// `directory`, the one its linked worktrees share, and `directory`'s path
/// beneath that working tree, or its refusal when that is not a logical
/// path. A directory inside the git directory is in no working tree, as
/// `git rev-parse --show-prefix` refuses there.
fn holding(directory: &Path) -> Option<(PathBuf, Result<LogicalPath, PathError>)> {
    let git = gix::discover(directory).ok()?;
    let common = git.common_dir().canonicalize().ok()?;
    let directory = directory.canonicalize().ok()?;
    if directory.starts_with(&common) {
        return None;
    }
    let beneath = directory
        .strip_prefix(git.workdir()?.canonicalize().ok()?)
        .ok()?;
    let path = format!("/{}", beneath.to_string_lossy());
    let reason = Malformation::NotUtf8;
    let within = match beneath.to_str() {
        Some(_) => path.parse(),
        None => Err(PathError::Malformed { path, reason }),
    };
    Some((common, within))
}

/// A mount pointer in the working tree, ready to move to its `follow`.
#[derive(Debug)]
pub struct Pin {
    pub target: Target,
    /// The pointer's blob now.
    pub blob: ObjectId,
    pub old: ObjectId,
    /// The ref `new` was read from, as the pointer names it.
    pub follow: String,
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

    pub fn fields(&self) -> Fields {
        match self {
            Self::NotAMount { path } | Self::Mount { path, .. } => Fields::at(path),
            Self::Write(error) => error.fields(),
        }
    }
}

impl<F> Reached<F> {
    /// Records the mounts `found` in `source` of the repository at `from`,
    /// whose route is `route`, and adds each one that can be read as a
    /// candidate.
    fn add(
        &mut self,
        repositories: &Repositories,
        from: &Path,
        source: &Source,
        route: &str,
        found: Vec<(LogicalPath, String, Mount)>,
        candidates: &mut Vec<Candidate>,
    ) {
        for (at, path, mount) in found {
            let mounted = repositories
                .mounted(from, &mount)
                .ok_or(Code::MountRepoMissing)
                .and_then(|mounted| {
                    let root = mounted
                        .commit_tree(mount.commit)
                        .map_err(|_| Code::MountCommitMissing)?
                        .root();
                    Ok((mounted, root))
                });
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
    let entries = walk_stored(&repository, commit, root, &mut |tree| cache.rows(tree))?;
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
/// in the source, and what it mounts. `is_pointer` says, from a cache, which
/// physical paths hold a pointer that parses.
fn pointers(
    repository: &Repository,
    source: &Source,
    entries: &[Entry],
    is_pointer: &dyn Fn(&str) -> bool,
) -> Vec<(LogicalPath, String, Mount)> {
    let reader = &mut repository.reader();
    entries
        .iter()
        .filter(|entry| entry.status == Status::Ok && is_pointer(&entry.path))
        .filter_map(|entry| {
            let contents = reader.read(source, &entry.path, entry.blob?).ok()??;
            match board_model::pointer(&contents.bytes) {
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
