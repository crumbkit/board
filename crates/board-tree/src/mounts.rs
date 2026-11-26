//! The boards reachable through mounts (docs/architecture/paths.md, Pointers:
//! links and mounts; docs/architecture/index.md). Each mounted commit is one source, walked
//! once however many places mount it.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use board_model::{
    Code, Digest, Fields, LogicalPath, Malformation, Mount, ObjectId, PathError, Pointer, Repo,
    Source,
};

use crate::git::{Contents, GitError, Identity, Repository, latest, remote_url};
use crate::repositories::Repositories;

use crate::trees::{TreeRow, TreeRows, walk_stored};
use crate::walk::{Entry, Status};
use crate::write::{Target, WriteError};

mod choice;
mod keys;
mod probes;

pub use choice::Choice;
use keys::{Listing, Probe};
pub use probes::Probed;

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
    /// What opening a mounted repository found, kept under `stamp`, or
    /// `None` when not held.
    fn probe(&mut self, stamp: &Digest) -> Option<Probed>;
    fn put_probe(&mut self, stamp: &Digest, probed: &Probed) -> Result<(), Self::Error>;
}

/// An entry holding a pointer that parses, as a walk listed it, with the
/// mount it holds once read, or `None` before or when it holds none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Held {
    pub path: String,
    pub logical: LogicalPath,
    pub blob: ObjectId,
    pub mount: Option<Mount>,
}

/// A mounted commit, its entries and the cache's facts about them.
#[derive(Debug)]
pub struct Mounted<F> {
    pub commit: ObjectId,
    pub root: ObjectId,
    /// The path of the repository it was first read from, as
    /// [`Repository::path`] gives it, which is opened only when its
    /// objects are read. A relative `repo` in one of its own mounts is read
    /// against this repository.
    pub repository: PathBuf,
    /// The display path of its first route, by shortest display path and
    /// then lexically; its entries are named beneath it.
    pub route: LogicalPath,
    /// Everything its decision depends on, hashed, or `None` when that is
    /// not known without deciding it. When the cache holds its decision
    /// under `lacking`, that key.
    pub key: Option<Digest>,
    /// The key its decision is kept under when a file in it cannot be
    /// read because its repository lacks the blob: `key` and `objects`.
    /// Found only when the cache does not hold its decision under `key`.
    pub lacking: Option<Digest>,
    /// The stamp of its repository's objects, taken before any walk of it
    /// read them, which a blob arriving there changes. `None` when it was
    /// not needed, or when a walk could not read a file whose blob the
    /// repository holds, which may be read next run.
    pub objects: Option<Digest>,
    /// The key of its list of pointers, which for a list made while a file
    /// could not be read also holds its repository's objects.
    pub scan: Digest,
    /// Each mount its decision may cross because its key holds that
    /// mount's probe.
    pub crossings: HashSet<Crossing>,
    /// Whether the cache holds its decision under `key`.
    pub kept: bool,
    /// Whether the scope holds all of it on some route.
    pub whole: bool,
    /// Its entries and the cache's facts about them, or `None` when it was
    /// not walked (see `walk_mounts`); discovery may walk a source the
    /// scope does not reach.
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
    /// Each mount pointer, by its source and physical path, whose repository
    /// is one a board above it on its route is in, with that board's route.
    /// It is degraded with `board::mount::cycle` and not walked.
    pub cycles: HashMap<(Source, String), LogicalPath>,
}

/// A mount found but not yet walked.
struct Candidate {
    route: LogicalPath,
    commit: ObjectId,
    root: ObjectId,
    repository: PathBuf,
    chain: Chain,
}

/// The repository of each board on a route, from the root's down, with
/// the board's route.
type Chain = Vec<(Identity, LogicalPath)>;

impl Repositories {
    /// Every board reachable through mounts from the board's working tree,
    /// whose entries are `entries` with the cache's `facts` about them. Each
    /// mounted commit's trees that `cache` does not hold are stored in it,
    /// then walked from it. A repository that several pointers name is
    /// opened once. A source whose pointers `cache` lists is walked only
    /// when `scope` reaches it. When `skip_kept` is true, such a source is
    /// also not walked when its decision is kept and `scope` holds all of
    /// it. `build` names the code that decides, for the keys. With an empty
    /// `build`, nothing is read from or stored in the cache's pointer lists
    /// or kept decisions.
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
            cycles: HashMap::new(),
        };
        let keeping = !build.is_empty();
        let mut candidates = Vec::new();
        let root = self.root();
        let shared = keeping.then(|| probes::shared(root)).flatten();
        let is_pointer = |path: &str| C::is_pointer(facts, path);
        let found = pointers(root, &Source::WorkingTree, entries, &is_pointer);
        let from = root.path();
        let route = LogicalPath::root();
        let chain = vec![(root.identity(), route.clone())];
        let at = (&Source::WorkingTree, &route, chain.as_slice());
        let probing = (&mut *cache, shared.as_deref());
        reached.add((self, probing), from, at, found, &mut candidates)?;
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
                chain,
            } = candidate;
            let source = Source::Commit(commit);
            let Listing {
                scan,
                objects,
                walked,
                held,
                unread,
            } = self.listing(cache, (build, keeping), (commit, root, &repository))?;
            if unread {
                reached.unread.push(source.clone());
            }
            let probing = (&mut *cache, shared.as_deref());
            reached.add(
                (self, probing),
                &repository,
                (&source, &route, &chain),
                mounts(&held),
                &mut candidates,
            )?;
            reached.sources.push(Mounted {
                commit,
                root,
                repository,
                route,
                key: None,
                lacking: None,
                objects,
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
        reached.kept(cache);
        for mounted in &mut reached.sources {
            let states = reached.scopes.get(&Source::Commit(mounted.commit));
            mounted.whole = states.is_some_and(|states| states.contains(&None));
            let wanted = states.is_some();
            let skip = skip_kept && mounted.kept && mounted.whole;
            if mounted.walked.is_none() && wanted && !skip {
                let opened = self.open_at(&mounted.repository)?;
                let (entries, facts, absent) = walk(&opened, mounted.commit, mounted.root, cache)?;
                if !absent {
                    (mounted.objects, mounted.lacking) = (None, None);
                }
                mounted.walked = Some((entries, facts));
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
        let (target, Contents { blob, bytes }, pointer) =
            self.pointer_at(path)?.ok_or_else(not_a_mount)?;
        let Pointer::Mount(mount) = pointer else {
            return Err(not_a_mount());
        };
        let root = self.root();
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
    /// The physical path of the file in the working tree that holds the
    /// mount or repo record, such as `repos/lib.json`.
    pub pointer: String,
    pub within: LogicalPath,
    pub matched: Matched,
    pub choice: Choice,
}

/// How a place was found, best first within one [`Choice`]: a mount of the directory's
/// repository; a mount of another clone with the same origin whose commit
/// the directory's repository holds; one whose commit it does not hold; or
/// a repo record whose `remote` is that repository's origin.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Matched {
    Mount,
    OriginWithCommit,
    Origin,
    Remote,
}

impl Place {
    /// The directory's logical path in the board, such as
    /// `/repos/crumbkit/docs`.
    pub fn prefix(&self) -> Result<LogicalPath, PathError> {
        self.mount.beneath(&self.within)
    }
}

impl Repositories {
    /// The directory of each repository that a mount among `entries`, the
    /// working tree's, names: its `repo` read against the root's directory,
    /// or against the home directory for `~/`. A `~/` path with no home is
    /// left out. `is_pointer` says, from a cache,
    /// which physical paths hold a pointer that parses.
    pub fn mounted_repositories(
        &self,
        entries: &[Entry],
        is_pointer: &dyn Fn(&str) -> bool,
    ) -> Vec<PathBuf> {
        let root = self.root();
        let mounts = pointers(root, &Source::WorkingTree, entries, is_pointer);
        let named = |(_, _, mount): &(_, _, Mount)| self.named(root.path(), mount);
        mounts.iter().filter_map(named).collect()
    }

    /// The mounts among `entries`, the working tree's, that hold
    /// `directory`, of the smallest [`Place::rank`]. A mount holds it when
    /// its repository is the directory's, compared by the git directory a
    /// linked worktree shares with its main one, or is a separate clone
    /// with the same `origin` in its stored form. A mount beneath a record
    /// whose `phase` is `done` or `dropped` is left out. When no mount
    /// holds it, each repo record whose `remote` is the stored form of that
    /// `origin`, at its own path. `is_pointer` says, from a cache, which
    /// physical paths hold a pointer that parses, and `is_repo` which hold
    /// a valid repo record. Each call opens and compares every mounted
    /// repository again.
    pub fn places(
        &self,
        entries: &[Entry],
        is_pointer: &dyn Fn(&str) -> bool,
        is_repo: &dyn Fn(&str) -> bool,
        directory: &Path,
    ) -> Vec<Place> {
        let Some((common, Ok(within))) = holding(directory) else {
            return Vec::new();
        };
        let root = self.root();
        let remote = match origin(directory) {
            Origin::Url(origin) => Some(board_model::shown_remote(&origin)),
            Origin::Outside | Origin::Unread(_) => None,
        };
        let clone = gix::discover(directory).ok();
        let branch = clone.as_ref().and_then(choice::branch);
        let matched = |mount: &Mount| {
            let mounted = self.mounted(root.path(), mount)?;
            if mounted.git().common_dir().canonicalize().ok().as_ref() == Some(&common) {
                return Some(Matched::Mount);
            }
            let url = remote_url(mounted.git()).ok()?;
            (Some(board_model::shown_remote(&url)) == remote).then_some(())?;
            let id = gix::ObjectId::try_from(mount.commit.as_bytes()).ok();
            let holds = (clone.as_ref().zip(id)).is_some_and(|(git, id)| git.has_object(id));
            Some(if holds {
                Matched::OriginWithCommit
            } else {
                Matched::Origin
            })
        };
        let reader = &mut root.reader();
        let mounts = pointers(root, &Source::WorkingTree, entries, is_pointer);
        let mounted: Vec<Place> = (mounts.iter())
            .filter_map(|(at, pointer, mount)| {
                let matched = matched(mount)?;
                let choice = choice::of(entries, reader, at, &mount.follow, branch.as_deref())?;
                Some(Place {
                    mount: at.clone(),
                    pointer: pointer.clone(),
                    within: within.clone(),
                    matched,
                    choice,
                })
            })
            .collect();
        if !mounted.is_empty() {
            return choice::best(mounted);
        }
        let Some(remote) = remote else {
            return Vec::new();
        };
        let named = |entry: &Entry| {
            let blob = entry
                .blob
                .filter(|_| entry.status == Status::Ok && is_repo(&entry.path))?;
            let read = reader
                .read(&Source::WorkingTree, &entry.path, blob)
                .ok()??;
            let value = board_model::parse_json(&read.bytes).ok()?;
            (Repo::from_value(&value).ok()?.remote == remote).then_some(())?;
            Some(Place {
                mount: entry.logical.clone()?,
                pointer: entry.path.clone(),
                within: LogicalPath::root(),
                matched: Matched::Remote,
                choice: Choice::Other,
            })
        };
        entries.iter().filter_map(named).collect()
    }
}

/// What the repository holding a directory says its `origin` remote is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Origin {
    /// The directory is in no git repository that can be opened.
    Outside,
    /// The URL `git remote get-url origin` gives, with any `insteadOf`
    /// applied.
    Url(String),
    /// The repository gives no URL, and why, as git says it, such as
    /// `error: No such remote 'origin'`.
    Unread(String),
}

/// The `origin` remote of the repository holding `directory`, read from
/// git's config without starting git. A repository git would refuse, for
/// its owner or a config that does not parse, is unread.
pub fn origin(directory: &Path) -> Origin {
    read_origin(directory, false)
}

/// The `origin` remote of the repository at `directory` itself, as git
/// reads it with `GIT_CEILING_DIRECTORIES` set to the directory's parent:
/// a directory that is not a repository is outside, even inside another
/// repository's working tree.
pub fn own_origin(directory: &Path) -> Origin {
    read_origin(directory, true)
}

/// [`origin`], or with `own` [`own_origin`].
fn read_origin(directory: &Path, own: bool) -> Origin {
    // Finding the repository reads no config, so only a directory in no
    // repository, or one that cannot be read, is outside.
    let Ok((found, trust)) = gix::discover::upwards(directory) else {
        return Origin::Outside;
    };
    if own && !is_root(&found, directory) {
        return Origin::Outside;
    }
    if trust != gix::sec::Trust::Full {
        // git refuses a repository another user owns; gix would read it
        // with reduced trust.
        let (git_dir, _) = found.into_repository_and_work_tree_directories();
        return Origin::Unread(format!(
            "fatal: detected dubious ownership in repository at '{}'",
            git_dir.display()
        ));
    }
    let git = match gix::ThreadSafeRepository::discover(directory) {
        Ok(git) => git.to_thread_local(),
        Err(error) => return Origin::Unread(format!("error: {error}")),
    };
    match remote_url(&git) {
        Ok(url) => Origin::Url(url),
        Err(reason) => Origin::Unread(reason),
    }
}

/// Whether `found` is the repository at `directory` itself: its working
/// tree, or its git directory when it has none. Both sides are canonical,
/// since a temporary directory under /var is /private/var on macOS.
fn is_root(found: &gix::discover::repository::Path, directory: &Path) -> bool {
    use gix::discover::repository::Path as Found;
    let root = match found {
        Found::WorkTree(work_dir) | Found::LinkedWorkTree { work_dir, .. } => work_dir,
        Found::Repository(git_dir) => git_dir,
    };
    let canonical = |path: &Path| path.canonicalize().ok();
    canonical(root).is_some_and(|root| Some(root) == canonical(directory))
}

/// `directory`'s path beneath the working tree of the repository holding
/// it, as `git rev-parse --show-prefix` gives it, such as `/docs`, or the
/// refusal of that path when it is not a logical path.
pub fn within(directory: &Path) -> Option<Result<LogicalPath, PathError>> {
    holding(directory).map(|(_, within)| within)
}

/// The commit HEAD names in the working tree holding `directory`, which in
/// a linked worktree is that worktree's own, or `None` when `directory` is
/// in no repository or HEAD names no commit yet.
pub fn head_of(directory: &Path) -> Option<ObjectId> {
    let git = gix::discover(directory).ok()?;
    let id = git.head_id().ok()?.detach();
    Some(crate::git::model_id(&id))
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
/// `cache`, and gets the cache's facts about its entries, and whether each
/// file the facts leave out names a blob the repository does not hold, so
/// it was not read because it is absent.
fn walk<C: Cache>(
    repository: &Repository,
    commit: ObjectId,
    root: ObjectId,
    cache: &mut C,
) -> Result<(Vec<Entry>, C::Facts, bool), C::Error> {
    for tree in repository.trees(root, &mut |tree| cache.has_tree(tree))? {
        cache.put_tree(&tree)?;
    }
    let entries = walk_stored(repository, commit, root, &mut |tree| cache.rows(tree))?;
    let facts = cache.facts(repository, &Source::Commit(commit), &entries)?;
    let left_out = |entry: &&Entry| !C::complete(&facts, std::slice::from_ref(*entry));
    let lacks =
        |blob: ObjectId| (repository.gix_id(blob)).is_ok_and(|id| !repository.git().has_object(id));
    let absent = (entries.iter().filter(left_out)).all(|entry| entry.blob.is_some_and(lacks));
    Ok((entries, facts, absent))
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
                mount: None,
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
    let mut held = held.to_vec();
    let unread = read_mounts(repository, source, &mut held);
    (mounts(&held), unread)
}

/// Each mount among `held`, with its logical and physical paths.
fn mounts(held: &[Held]) -> Vec<(LogicalPath, String, Mount)> {
    (held.iter())
        .filter_map(|held| Some((held.logical.clone(), held.path.clone(), held.mount.clone()?)))
        .collect()
}

/// Reads the mount each of `held`, pointers in `source`, holds, and says
/// whether the blob of one could not be read.
fn read_mounts(repository: &Repository, source: &Source, held: &mut [Held]) -> bool {
    let reader = &mut repository.reader();
    let mut unread = false;
    for held in held {
        // A pointer rewritten since the walk is the mount it now says;
        // only a commit's pointers cannot change, so only the working
        // tree's can differ.
        let read = latest(reader.read(source, &held.path, held.blob));
        unread |= read.is_err();
        held.mount = match read
            .ok()
            .flatten()
            .map(|read| board_model::pointer(&read.bytes))
        {
            Some(Ok(Some(Pointer::Mount(mount)))) => Some(mount),
            _ => None,
        };
    }
    unread
}
