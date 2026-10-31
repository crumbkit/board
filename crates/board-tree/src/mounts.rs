//! The boards reachable through mounts (docs/architecture/paths.md, Pointers:
//! links and mounts; docs/architecture/index.md). Each mounted commit is one source, walked
//! once however many places mount it.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use board_model::{
    Code, Digest, Fields, LogicalPath, Malformation, Mount, Object, ObjectId, PathError, Pointer,
    Source,
};

use crate::git::{Contents, GitError, Repository, latest};
use crate::repositories::Repositories;
use crate::tree::Tree;
use crate::trees::{TreeRow, TreeRows, walk_stored};
use crate::walk::{Entry, Status};
use crate::write::{Target, WriteError};

mod keys;

use keys::{Probe, scan};

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
    /// Whether `facts` hold every entry of `entries` that has a role, so
    /// that the pointers found among them are all there are.
    fn complete(facts: &Self::Facts, entries: &[Entry]) -> bool;
    /// The pointers a walk listed under `scan`, or `None` when not held.
    fn pointers(&mut self, scan: &Digest) -> Option<Vec<Held>>;
    fn put_pointers(&mut self, scan: &Digest, held: &[Held]) -> Result<(), Self::Error>;
    /// Whether the cache holds the decision of the source keyed `key`.
    fn kept(&mut self, key: &Digest) -> bool;
}

/// An entry holding a pointer that parses, as a walk listed it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Held {
    pub path: String,
    pub logical: LogicalPath,
    pub blob: ObjectId,
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
    pub route: LogicalPath,
    /// Everything its decision depends on, hashed, or `None` when that is
    /// not known without deciding it.
    pub key: Option<Digest>,
    /// The key of its list of pointers.
    pub scan: Digest,
    /// Each mount its decision may cross because its key holds that
    /// mount's probe.
    pub crossings: HashSet<Crossing>,
    /// Whether the cache holds its decision under `key`.
    pub kept: bool,
    /// Whether the scope holds all of it on some route.
    pub whole: bool,
    /// Its entries and the cache's facts about them, or `None` when it was
    /// not walked: the scope does not reach it, or it is kept, the scope
    /// holds all of it, and the walk was asked to skip what is kept. A
    /// source that discovery walked to find its mounts holds them even when
    /// the scope does not reach it.
    pub walked: Option<(Vec<Entry>, F)>,
}

/// A mount a resolution crossed or tried to: the repository holding the
/// pointer, the pointer's `repo`, and its commit.
pub type Crossing = (PathBuf, String, ObjectId);

impl<F> Mounted<F> {
    /// The display path of `logical`, a path inside this source.
    pub fn display(&self, logical: &LogicalPath) -> Option<LogicalPath> {
        self.route.beneath(logical).ok()
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
    probes: Vec<Probe>,
    /// Each source holding a pointer whose blob could not be read.
    unread: Vec<Source>,
    /// What remains of the scope at each source's root.
    pub scopes: Scopes,
}

/// A mount found but not yet walked.
struct Candidate {
    route: LogicalPath,
    commit: ObjectId,
    root: ObjectId,
    repository: Repository,
}

impl Repositories {
    /// Every board reachable through mounts from the board's working tree,
    /// whose entries are `entries` with the cache's `facts` about them. Each
    /// mounted commit's trees that `cache` does not hold are stored in it,
    /// then walked from it. A repository that several pointers name is
    /// opened once. A source whose pointers `cache` lists is walked only
    /// when `scope` reaches it, and, when `skip_kept`, its decision is not
    /// kept or `scope` holds only part of it. `build` names the code that
    /// decides, for the
    /// keys; with an empty `build` nothing is read from or stored in the
    /// cache's pointer lists or kept decisions.
    pub fn walk_mounts<C: Cache>(
        &self,
        entries: &[Entry],
        facts: &C::Facts,
        cache: &mut C,
        (build, skip_kept): (&str, bool),
        scope: Option<&LogicalPath>,
    ) -> Result<Reached<C::Facts>, C::Error> {
        let mut reached = Reached {
            sources: Vec::new(),
            points: Vec::new(),
            edges: Vec::new(),
            probes: Vec::new(),
            unread: Vec::new(),
            scopes: HashMap::new(),
        };
        let keeping = !build.is_empty();
        let mut candidates = Vec::new();
        let root = self.root();
        let is_pointer = |path: &str| C::is_pointer(facts, path);
        let found = pointers(root, &Source::WorkingTree, entries, &is_pointer);
        let from = root.path();
        let route = LogicalPath::root();
        reached.add(
            self,
            from,
            &Source::WorkingTree,
            &route,
            found,
            &mut candidates,
        );
        let mut seen: HashSet<ObjectId> = HashSet::new();
        while let Some(at) = first(&candidates) {
            let candidate = candidates.swap_remove(at);
            if !seen.insert(candidate.commit) {
                continue;
            }
            let Candidate {
                route,
                commit,
                root,
                repository,
            } = candidate;
            let source = Source::Commit(commit);
            let scan = scan(build, root);
            let listed = if keeping { cache.pointers(&scan) } else { None };
            let (walked, held) = if let Some(held) = listed {
                (None, held)
            } else {
                let (entries, facts) = walk(&repository, commit, root, cache)?;
                let held = held(&entries, &|path| C::is_pointer(&facts, path));
                if keeping && C::complete(&facts, &entries) {
                    cache.put_pointers(&scan, &held)?;
                }
                (Some((entries, facts)), held)
            };
            let (found, unread) = mounts_in(&repository, &source, &held);
            if unread {
                reached.unread.push(source.clone());
            }
            let from = repository.path();
            reached.add(self, from, &source, &route, found, &mut candidates);
            reached.sources.push(Mounted {
                commit,
                root,
                repository,
                route,
                key: None,
                scan,
                crossings: HashSet::new(),
                kept: false,
                whole: false,
                walked,
            });
        }
        if keeping {
            reached.key(build);
        }
        reached.scopes = reached.scopes(scope);
        for mounted in &mut reached.sources {
            mounted.kept = mounted.key.as_ref().is_some_and(|key| cache.kept(key));
            let states = reached.scopes.get(&Source::Commit(mounted.commit));
            mounted.whole = states.is_some_and(|states| states.contains(&None));
            let wanted = states.is_some();
            let skip = skip_kept && mounted.kept && mounted.whole;
            if mounted.walked.is_none() && wanted && !skip {
                let walked = walk(&mounted.repository, mounted.commit, mounted.root, cache)?;
                mounted.walked = Some(walked);
            }
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
        if !board_model::can_hold_pointer(&target.path) {
            return Err(not_a_mount());
        }
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
    pub fn prefix(&self) -> Result<LogicalPath, PathError> {
        self.mount.beneath(&self.within)
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
    /// whose route is `route`, and adds each one that can be read, and whose
    /// route is a logical path, as a candidate.
    fn add(
        &mut self,
        repositories: &Repositories,
        from: &Path,
        source: &Source,
        route: &LogicalPath,
        found: Vec<(LogicalPath, String, Mount)>,
        candidates: &mut Vec<Candidate>,
    ) {
        let wanted: Vec<_> = (found.iter())
            .filter(|(at, _, _)| route.beneath(at).is_ok())
            .filter_map(|(_, _, mount)| Some((repositories.named(from, mount)?, mount.commit)))
            .collect();
        repositories.open_all(&wanted);
        for (at, path, mount) in found {
            let route = route.beneath(&at).map_err(|_| Code::PathMalformed);
            let mounted = route.and_then(|route| {
                let mounted = (repositories.mounted(from, &mount)).ok_or(Code::MountRepoMissing)?;
                let tree = mounted.commit_tree(mount.commit);
                let root = tree.map_err(|_| Code::MountCommitMissing)?.root();
                Ok((route, mounted, root))
            });
            self.probes.push(Probe {
                from: source.clone(),
                path: path.clone(),
                repo: mount.repo.clone(),
                commit: mount.commit,
                landed: (mounted.as_ref())
                    .map(|(_, mounted, _)| mounted.path().to_path_buf())
                    .map_err(|code| *code),
            });
            self.points.push(MountPoint {
                source: source.clone(),
                path,
                target: mounted.as_ref().map(|_| mount.commit).map_err(|code| *code),
            });
            let Ok((route, mounted, root)) = mounted else {
                continue;
            };
            candidates.push(Candidate {
                route,
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
    fn scopes(&self, scope: Option<&LogicalPath>) -> Scopes {
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
        .min_by_key(|(_, candidate)| (candidate.route.as_str().len(), &candidate.route))
        .map(|(at, _)| at)
}

/// Stores a mounted commit's trees that `cache` does not hold, walks it from
/// `cache`, and gets the cache's facts about its entries.
fn walk<C: Cache>(
    repository: &Repository,
    commit: ObjectId,
    root: ObjectId,
    cache: &mut C,
) -> Result<(Vec<Entry>, C::Facts), C::Error> {
    for tree in repository.trees(root, &mut |tree| cache.has_tree(tree))? {
        cache.put_tree(&tree)?;
    }
    let entries = walk_stored(repository, commit, root, &mut |tree| cache.rows(tree))?;
    let facts = cache.facts(repository, &Source::Commit(commit), &entries)?;
    Ok((entries, facts))
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
    mounts_in(repository, source, &held(entries, is_pointer)).0
}

/// The entries among `entries` that hold a pointer that parses.
fn held(entries: &[Entry], is_pointer: &dyn Fn(&str) -> bool) -> Vec<Held> {
    entries
        .iter()
        .filter(|entry| entry.status == Status::Ok && is_pointer(&entry.path))
        .filter_map(|entry| {
            Some(Held {
                path: entry.path.clone(),
                logical: entry.logical.clone()?,
                blob: entry.blob?,
            })
        })
        .collect()
}

/// The mounts among the pointers `held` in `source`, and whether the blob
/// of one could not be read.
fn mounts_in(
    repository: &Repository,
    source: &Source,
    held: &[Held],
) -> (Vec<(LogicalPath, String, Mount)>, bool) {
    let reader = &mut repository.reader();
    let mut unread = false;
    let found = held
        .iter()
        .filter_map(|held| {
            let read = reader.read(source, &held.path, held.blob);
            unread |= read.is_err();
            match board_model::pointer(&read.ok()??.bytes) {
                Ok(Some(Pointer::Mount(mount))) => {
                    Some((held.logical.clone(), held.path.clone(), mount))
                }
                _ => None,
            }
        })
        .collect();
    (found, unread)
}
