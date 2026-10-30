//! Resolving a logical path to a Location (docs/architecture/paths.md, Pointers:
//! links and mounts). Pointers are followed with POSIX symlink semantics.

use std::collections::{HashSet, VecDeque};
use std::path::Path;
use std::sync::Arc;

use board_model::{
    Board, Code, Fields, Location, LogicalPath, Mount, Object, ObjectId, Pointer, PointerError,
    Source,
};

use crate::address::{Named, Pointers};
use crate::git::{CommitTree, Contents, GitError, Repository, Sizes};
use crate::repositories::Repositories;
use crate::symlink::{Chains, End, Node, ROOT, Refused};
use crate::tree::{Kind, RawEntry, Tree};
use crate::worktree::{Known, WorkingTree};

/// The most pointers one resolution follows, as POSIX allows symlinks.
pub const MAX_HOPS: usize = 40;

/// Where a path that names nothing would be: its deepest ancestor that
/// resolves, and the segments past it (docs/architecture/paths/writes.md).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ancestor {
    /// Where the ancestor resolved, with every pointer followed.
    pub location: Location,
    /// The segments of the path past the ancestor; at least one.
    pub rest: Vec<String>,
    /// The ancestor's own logical path.
    pub logical: LogicalPath,
}

impl Ancestor {
    /// The physical path the rest would have beneath the ancestor: in it,
    /// when it is a directory, or in the directory beside it, when it is a
    /// JSON file. `None` when it is any other file, which holds nothing.
    pub fn path(&self) -> Option<String> {
        let directory = match self.location.object {
            Object::Directory => self.location.path.as_str(),
            Object::Blob(_) => self.location.path.strip_suffix(".json")?,
        };
        let mut parts: Vec<&str> = Vec::new();
        if !directory.is_empty() {
            parts.push(directory);
        }
        parts.extend(self.rest.iter().map(String::as_str));
        Some(parts.join("/"))
    }
}

impl Repositories {
    /// Resolves `path` in `source` of the board's own repository, following
    /// every pointer on the way, including one at the end.
    pub fn resolve(&self, source: &Source, path: &LogicalPath) -> Result<Location, ResolveError> {
        match source {
            Source::Commit(commit) => self.resolve_in(self.root(), *commit, path),
            Source::WorkingTree => self.snapshot()?.resolve(path),
        }
    }

    /// Resolves `path` in `commit` of `repository`, which is the board's own
    /// or one its mounts name, as [`Repositories::resolve`] does.
    pub fn resolve_in(
        &self,
        repository: &Repository,
        commit: ObjectId,
        path: &LogicalPath,
    ) -> Result<Location, ResolveError> {
        self.in_commit(repository, commit)?.resolve(path)
    }

    /// A resolver for many paths in `commit` of `repository`, as
    /// [`Repositories::resolve_in`] resolves one.
    pub fn in_commit<'r>(
        &'r self,
        repository: &'r Repository,
        commit: ObjectId,
    ) -> Result<InCommit<'r>, GitError> {
        Ok(InCommit {
            repositories: self,
            tree: repository.commit_tree(commit)?.sizing(Sizes::Symlinks),
            pointers: Pointers::default(),
            chains: Chains::default(),
        })
    }

    /// The deepest ancestor of `path` that resolves in the working tree,
    /// as [`Resolver::ancestor`] finds it.
    pub fn ancestor(&self, path: &LogicalPath) -> Result<Ancestor, ResolveError> {
        self.snapshot()?.ancestor(path)
    }

    /// A resolver for many paths in one snapshot of the working tree, which
    /// hashes a file it picks or reads only when `known` does not already
    /// know its blob.
    pub fn resolver<'k>(&self, known: Known<'k>) -> Result<Resolver<'_, 'k>, GitError> {
        Ok(Resolver {
            repositories: self,
            tree: self.root().working_tree(Some(known))?.on_demand(),
            pointers: Pointers::default(),
            chains: Chains::default(),
        })
    }

    /// A resolver for many paths in one snapshot of the working tree, which
    /// hashes each file it picks or reads, once. A batch of writes checks every record
    /// against one snapshot, so it lists each directory once.
    pub fn snapshot(&self) -> Result<Resolver<'_, 'static>, GitError> {
        Ok(Resolver {
            repositories: self,
            tree: self.root().working_tree(None)?.on_demand(),
            pointers: Pointers::default(),
            chains: Chains::default(),
        })
    }

    pub(crate) fn resolve_tree<T: Tree>(
        &self,
        tree: &mut T,
        path: &LogicalPath,
        follow_last: bool,
        (pointers, chains): (&mut Pointers, &mut Chains<T::Directory>),
    ) -> Result<Location, ResolveError> {
        let mut trail = Trail {
            path: path.clone(),
            pointers: Vec::new(),
            visited: HashSet::new(),
            link: None,
            follow_last,
        };
        let segments = path.segments().map(str::to_owned).collect();
        self.follow(tree, segments, 0, &mut trail, (pointers, chains))
    }

    /// Resolves `remaining` from the root of `tree`, recording on `trail`
    /// every pointer followed. A mount
    /// continues in the mounted repository with what remains. The first
    /// `from_link` segments came from the last link's target, so a miss
    /// among them is that link dangling.
    fn follow<T: Tree>(
        &self,
        tree: &mut T,
        mut remaining: VecDeque<String>,
        mut from_link: usize,
        trail: &mut Trail,
        (pointers, chains): (&mut Pointers, &mut Chains<T::Directory>),
    ) -> Result<Location, ResolveError> {
        let path = &trail.path.clone();
        let board = Board {
            repository: tree.repository().to_path_buf(),
            source: tree.source(),
        };
        let mut directory = Some((tree.root(), ROOT));
        let mut physical: Vec<String> = Vec::new();
        let mut object = Object::Directory;
        while let Some(segment) = remaining.pop_front() {
            let missing = |from_link: usize, trail: &Trail| match &trail.link {
                Some((link, target)) if from_link > 0 => ResolveError::Dangling {
                    path: path.clone(),
                    link: link.clone(),
                    target: target.clone(),
                },
                _ => not_found(path, remaining.len()),
            };
            let Some((current, place)) = directory.take() else {
                return Err(missing(from_link, trail));
            };
            let entries = tree.alike(&current, segment.as_bytes(), Some(&segment))?;
            let Some(entry) = pick(
                tree,
                &current,
                &entries,
                segment,
                remaining.is_empty(),
                path,
                pointers,
            )?
            else {
                return Err(missing(from_link, trail));
            };
            let entry = tree.settle(&current, entry);
            from_link = usize::saturating_sub(from_link, 1);
            physical.push(String::from_utf8_lossy(&entry.name).into_owned());
            let reached = path.segments().count().saturating_sub(remaining.len());
            let entered = chains.enter(tree, place, &entry);
            let (entry, at) = &through(tree, chains, entered, entry, &physical, (path, reached))?;
            match &entry.kind {
                Kind::Directory(child) => {
                    object = Object::Directory;
                    directory = Some((child.clone(), entered));
                }
                Kind::File(id) => {
                    object = Object::Blob(*id);
                    if remaining.is_empty() && !trail.follow_last {
                        continue;
                    }
                    let Some(next) = pointer(tree, (chains, *at), entry, path, pointers)? else {
                        continue;
                    };
                    trail.pointers.push(Location {
                        board: board.clone(),
                        path: physical.join("/"),
                        object,
                        pointers: Vec::new(),
                    });
                    if trail.pointers.len() > MAX_HOPS {
                        return Err(ResolveError::TooManyHops { path: path.clone() });
                    }
                    match next {
                        Pointer::Link(target) => {
                            let next: VecDeque<String> = target
                                .segments()
                                .map(str::to_owned)
                                .chain(remaining.drain(..))
                                .collect();
                            if !trail.visited.insert((board.clone(), next.clone())) {
                                return Err(ResolveError::Cycle { path: path.clone() });
                            }
                            from_link = target.segments().count();
                            trail.followed_link(target);
                            remaining = next;
                            directory = Some((tree.root(), ROOT));
                            physical.clear();
                            object = Object::Directory;
                        }
                        Pointer::Mount(mount) => {
                            let from = &board.repository;
                            return self.enter(from, &mount, remaining, from_link, trail, pointers);
                        }
                    }
                }
                Kind::Symlink(id) => object = Object::Blob(*id),
                Kind::Submodule => {
                    return Err(ResolveError::Submodule { path: path.clone() });
                }
                Kind::Unreadable => {
                    return Err(ResolveError::Unreadable { path: path.clone() });
                }
                Kind::Unhashed(_) => unreachable!("resolution settles each entry it picks"),
            }
        }
        Ok(Location {
            board,
            path: physical.join("/"),
            object,
            pointers: std::mem::take(&mut trail.pointers),
        })
    }

    /// The commit the repository `mount`, a pointer in the repository at
    /// `from`, names now at its `follow` ref. A missing repository is
    /// `mount::repo_missing`; a ref that is missing, not a valid name, or not
    /// peeled to a commit is `mount::follow_missing`.
    pub(crate) fn followed(&self, from: &Path, mount: &Mount) -> Result<ObjectId, Code> {
        let mounted = self.mounted(from, mount).ok_or(Code::MountRepoMissing)?;
        let mut reference = mounted
            .git()
            .try_find_reference(mount.follow.as_str())
            .ok()
            .flatten()
            .ok_or(Code::MountFollowMissing)?;
        let commit = reference
            .peel_to_commit()
            .map_err(|_| Code::MountFollowMissing)?;
        Ok(crate::git::model_id(&commit.id))
    }

    /// Continues with `remaining` at the root of the tree `mount`, a pointer
    /// in the repository at `from`, names.
    fn enter(
        &self,
        from: &Path,
        mount: &Mount,
        remaining: VecDeque<String>,
        from_link: usize,
        trail: &mut Trail,
        pointers: &mut Pointers,
    ) -> Result<Location, ResolveError> {
        let at = trail.last_pointer();
        let mounted = self
            .mounted(from, mount)
            .ok_or_else(|| ResolveError::RepoMissing {
                path: at.clone(),
                repo: mount.repo.clone(),
            })?;
        let key = (mounted.path().to_path_buf(), mount.commit);
        let (mut tree, mut chains) = if let Some((decoded, chains)) = pointers.entered.remove(&key)
        {
            (mounted.resume(mount.commit, decoded), chains)
        } else {
            let tree =
                mounted
                    .commit_tree(mount.commit)
                    .map_err(|_| ResolveError::CommitMissing {
                        path: at,
                        commit: mount.commit,
                    })?;
            (tree.sizing(Sizes::Symlinks), Chains::default())
        };
        let board = Board {
            repository: mounted.path().to_path_buf(),
            source: tree.source(),
        };
        // Mounts alone cannot cycle: each pins a commit, and no commit can
        // hold its own id. A cycle needs a link, and the hop limit bounds
        // the rest; this key catches a link cycle met through a mount.
        let followed = if trail.visited.insert((board, remaining.clone())) {
            let chains = &mut chains;
            self.follow(&mut tree, remaining, from_link, trail, (pointers, chains))
        } else {
            Err(ResolveError::Cycle {
                path: trail.path.clone(),
            })
        };
        pointers.entered.insert(key, (tree.decoded(), chains));
        followed
    }
}

/// Resolves paths in one snapshot of the working tree: each directory is
/// listed once, however many paths pass through it.
pub struct Resolver<'r, 'k> {
    repositories: &'r Repositories,
    tree: WorkingTree<'r, 'k>,
    pointers: Pointers,
    pub(crate) chains: Chains<std::path::PathBuf>,
}

impl std::fmt::Debug for Resolver<'_, '_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Resolver")
            .field("repository", &self.repositories.root().path())
            .finish_non_exhaustive()
    }
}

impl Resolver<'_, '_> {
    pub(crate) fn repository(&self) -> &Repository {
        self.repositories.root()
    }

    /// How many files this resolver has read to learn what they hold as a
    /// pointer. Work-count tests and the timing log read it.
    pub fn pointers_read(&self) -> usize {
        self.pointers.read
    }

    /// Resolves `path` as [`Repositories::resolve`] does in the working
    /// tree.
    pub fn resolve(&mut self, path: &LogicalPath) -> Result<Location, ResolveError> {
        self.repositories.resolve_tree(
            &mut self.tree,
            path,
            true,
            (&mut self.pointers, &mut self.chains),
        )
    }

    /// The file at the `/`-separated `path` in `board`, whose blob was
    /// `blob` when it was walked, as [`Reader::read`](crate::Reader::read)
    /// reads it, with a symlink in this working tree followed through this
    /// snapshot. A board in the working tree is the root repository's, as a
    /// snapshot has only that one.
    pub fn read(
        &mut self,
        board: &Board,
        path: &str,
        blob: ObjectId,
    ) -> Result<Option<Contents>, GitError> {
        if board.source != Source::WorkingTree {
            return self
                .repositories
                .repository(board)?
                .reader()
                .read(&board.source, path, blob);
        }
        let root = self.repositories.root();
        let snapshot = (&mut self.tree, &mut self.chains);
        root.read_walked(move || Ok(snapshot), Path::new(path), Some(blob))
    }

    /// Resolves `path` as [`Resolver::resolve`] does, except that a pointer
    /// at the end is not followed, as `lstat` does not follow a symlink.
    pub fn resolve_pointer(&mut self, path: &LogicalPath) -> Result<Location, ResolveError> {
        self.repositories.resolve_tree(
            &mut self.tree,
            path,
            false,
            (&mut self.pointers, &mut self.chains),
        )
    }

    /// What [`crate::walk::planned_code`] gives `file` in this snapshot.
    pub(crate) fn planned_code(
        &mut self,
        file: &str,
        is_pointer: bool,
    ) -> Result<Option<crate::walk::Planned>, GitError> {
        crate::walk::planned_code(&mut self.tree, file, is_pointer)
    }

    /// The deepest ancestor of `path` that resolves, for a `path` that names
    /// nothing. Any code but `path::not_found`, at an ancestor, is returned
    /// as it is.
    pub fn ancestor(&mut self, path: &LogicalPath) -> Result<Ancestor, ResolveError> {
        let segments: Vec<&str> = path.segments().collect();
        for depth in (0..segments.len()).rev() {
            let ancestor = path.prefix(depth);
            match self.resolve(&ancestor) {
                Ok(location) => {
                    return Ok(Ancestor {
                        location,
                        rest: segments[depth..]
                            .iter()
                            .map(|segment| (*segment).to_owned())
                            .collect(),
                        logical: ancestor,
                    });
                }
                Err(ResolveError::NotFound { .. }) => {}
                Err(error) => return Err(error),
            }
        }
        Err(ResolveError::NotFound {
            path: path.clone(),
            ancestor: LogicalPath::root(),
        })
    }
}

/// Resolves paths in one commit: each directory is decoded once, and each
/// symlink followed once, however many paths pass through it.
#[derive(Debug)]
pub struct InCommit<'r> {
    repositories: &'r Repositories,
    tree: CommitTree<'r>,
    pointers: Pointers,
    pub(crate) chains: Chains<ObjectId>,
}

impl InCommit<'_> {
    /// How many files this resolver has read to learn what they hold as a
    /// pointer.
    pub fn pointers_read(&self) -> usize {
        self.pointers.read
    }

    pub fn resolve(&mut self, path: &LogicalPath) -> Result<Location, ResolveError> {
        self.repositories.resolve_tree(
            &mut self.tree,
            path,
            true,
            (&mut self.pointers, &mut self.chains),
        )
    }
}

/// What one resolution carries across links and mounts.
struct Trail {
    /// The path being resolved, for errors.
    path: LogicalPath,
    pointers: Vec<Location>,
    /// Each board and path resolution restarted at.
    visited: HashSet<(Board, VecDeque<String>)>,
    /// The display path of the last link followed, and its target.
    link: Option<(LogicalPath, LogicalPath)>,
    /// Whether a pointer at the end is followed.
    follow_last: bool,
}

impl Trail {
    /// Records the last pointer followed as a link to `target`, named by
    /// its display path.
    fn followed_link(&mut self, target: LogicalPath) {
        self.link = Some((self.last_pointer(), target));
    }

    /// The display path of the last pointer followed, or the path being
    /// resolved if it has none.
    fn last_pointer(&self) -> LogicalPath {
        let at = self.pointers.split_last().and_then(|(pointer, before)| {
            Location {
                pointers: before.to_vec(),
                ..pointer.clone()
            }
            .display_path()
        });
        at.unwrap_or_else(|| self.path.clone())
    }
}

/// The entry `segment` names in `entries`, those of `directory` alike to
/// it, or `None` when it names nothing. A record beside its directory of
/// children is the record when `last`, else the directory, unless it
/// shadows the directory.
fn pick<'e, T: Tree>(
    tree: &mut T,
    directory: &T::Directory,
    entries: &'e [RawEntry<T::Directory>],
    segment: String,
    last: bool,
    path: &LogicalPath,
    pointers: &mut Pointers,
) -> Result<Option<&'e RawEntry<T::Directory>>, ResolveError> {
    match tree.named(directory, entries, &segment, pointers)? {
        Named::Nothing => Ok(None),
        Named::One(entry) => Ok(Some(entry)),
        Named::RecordWithChildren { record, children } => {
            Ok(Some(if last { record } else { children }))
        }
        Named::Shadowed => Err(ResolveError::Shadowed {
            path: path.clone(),
            segment,
        }),
    }
}

/// `entry`, or, for a symlink at the physical path `link` and the place
/// `at`, what it stands for: a file entry for the file it leads to, or
/// itself, a file holding its text, when it leads to a directory. `path` is
/// the path resolved, and `reached` how many of its segments reach the
/// symlink.
fn through<T: Tree>(
    tree: &mut T,
    chains: &mut Chains<T::Directory>,
    at: Node,
    mut entry: RawEntry<T::Directory>,
    link: &[String],
    (path, reached): (&LogicalPath, usize),
) -> Result<(RawEntry<T::Directory>, Node), ResolveError> {
    let Some((end, target)) = chains.follow(tree, at)? else {
        return Ok((entry, at));
    };
    match end {
        End::File { blob, at, .. } => {
            entry.kind = Kind::File(blob);
            return Ok((entry, at));
        }
        End::Directory(_) => {}
        End::Refused(Code::RepoUnreadable) => {
            return Err(ResolveError::Unreadable { path: path.clone() });
        }
        End::Refused(code) => {
            return Err(ResolveError::Symlink {
                path: path.clone(),
                reached,
                link: link.join("/").into(),
                refused: Refused { code, target },
            });
        }
    }
    Ok((entry, at))
}

/// The pointer `entry` holds, if it is a `.json` file whose record is one.
/// Its bytes are the file's at `at`: its own, or a symlink's target's.
fn pointer<T: Tree>(
    tree: &mut T,
    (chains, at): (&Chains<T::Directory>, Node),
    entry: &RawEntry<T::Directory>,
    path: &LogicalPath,
    pointers: &mut Pointers,
) -> Result<Option<Pointer>, ResolveError> {
    if !matches!(entry.kind, Kind::File(_)) || !entry.name.ends_with(b".json") {
        return Ok(None);
    }
    let parsed = pointers.parse(entry, || chains.read(tree, at))?;
    parsed.map_err(|source| ResolveError::InvalidPointer {
        path: path.clone(),
        pointer: String::from_utf8_lossy(&entry.name).into_owned(),
        source,
    })
}

/// `path` missing where `remaining` of its segments were left after the one
/// that named nothing. Past a link's target, what remains is the end of
/// `path`, so the segments of `path` before those resolved.
fn not_found(path: &LogicalPath, remaining: usize) -> ResolveError {
    let resolved = path.segments().count().saturating_sub(remaining + 1);
    ResolveError::NotFound {
        path: path.clone(),
        ancestor: path.prefix(resolved),
    }
}

/// A path that does not resolve.
#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    /// `ancestor` is the deepest ancestor of `path` that resolves.
    #[error("nothing is at {path}")]
    NotFound {
        path: LogicalPath,
        ancestor: LogicalPath,
    },
    #[error("{segment:?} in {path} names more than one entry")]
    Shadowed { path: LogicalPath, segment: String },
    #[error("{path} {}", self.predicate().unwrap_or_default())]
    Submodule { path: LogicalPath },
    #[error("{path} {}", self.predicate().unwrap_or_default())]
    Unreadable { path: LogicalPath },
    #[error("the link at {link} points to {target}, which is missing")]
    Dangling {
        path: LogicalPath,
        link: LogicalPath,
        target: LogicalPath,
    },
    #[error("{path} {}", self.predicate().unwrap_or_default())]
    InvalidPointer {
        path: LogicalPath,
        pointer: String,
        #[source]
        source: Arc<PointerError>,
    },
    #[error("{path} {}", self.predicate().unwrap_or_default())]
    RepoMissing { path: LogicalPath, repo: String },
    #[error("{path} {}", self.predicate().unwrap_or_default())]
    CommitMissing { path: LogicalPath, commit: ObjectId },
    #[error("{path} {}", self.predicate().unwrap_or_default())]
    Cycle { path: LogicalPath },
    #[error("{path} {}", self.predicate().unwrap_or_default())]
    TooManyHops { path: LogicalPath },
    /// `path` reaches the symlink at the physical `link`, which its first
    /// `reached` segments name, and which leads to no file.
    #[error("{path} {}", self.predicate().unwrap_or_default())]
    Symlink {
        path: LogicalPath,
        reached: usize,
        link: Box<str>,
        refused: Refused,
    },
    #[error(transparent)]
    Git(#[from] GitError),
}

impl ResolveError {
    /// What the message says of its path, for an error whose message starts
    /// with that path.
    pub(crate) fn predicate(&self) -> Option<String> {
        Some(match self {
            Self::Submodule { .. } => "reaches a submodule, which is not descended".to_owned(),
            Self::Unreadable { .. } => "reaches a file that cannot be read".to_owned(),
            Self::InvalidPointer {
                pointer, source, ..
            } => format!("reaches the invalid pointer {pointer:?}: {source}"),
            Self::RepoMissing { repo, .. } => {
                format!("mounts the repository {repo:?}, which cannot be opened")
            }
            Self::CommitMissing { commit, .. } => {
                format!("mounts commit {commit}, which its repository does not hold")
            }
            Self::Cycle { .. } => {
                "returns to a board and path it already resolved through a pointer".to_owned()
            }
            Self::TooManyHops { .. } => format!("follows more than {MAX_HOPS} pointers"),
            Self::Symlink {
                path,
                reached,
                refused,
                ..
            } if *reached < path.segments().count() => {
                format!("passes through {}, {refused}", path.prefix(*reached))
            }
            Self::Symlink { refused, .. } => format!("is {refused}"),
            Self::NotFound { .. }
            | Self::Shadowed { .. }
            | Self::Dangling { .. }
            | Self::Git(_) => {
                return None;
            }
        })
    }

    pub fn code(&self) -> Code {
        match self {
            Self::NotFound { .. } => Code::PathNotFound,
            Self::Shadowed { .. } => Code::PathShadowed,
            Self::Submodule { .. } => Code::RepoSubmodule,
            Self::Unreadable { .. } => Code::RepoUnreadable,
            Self::Dangling { .. } => Code::RefDangling,
            Self::InvalidPointer { source, .. } => source.code(),
            Self::RepoMissing { .. } => Code::MountRepoMissing,
            Self::CommitMissing { .. } => Code::MountCommitMissing,
            Self::Cycle { .. } => Code::ResolveCycle,
            Self::TooManyHops { .. } => Code::ResolveTooManyHops,
            Self::Symlink { refused, .. } => refused.code,
            Self::Git(error) => error.code(),
        }
    }

    /// The path at fault: the link for a dangling link, else the path.
    pub fn fields(&self) -> Fields {
        match self {
            Self::Dangling { link, .. } => Fields::at(link),
            Self::NotFound { path, ancestor } => Fields {
                ancestor: Some(ancestor.clone()),
                ..Fields::at(path)
            },
            Self::Symlink { path, refused, .. } => Fields {
                target: Some(refused.target.clone()),
                ..Fields::at(path)
            },
            Self::Shadowed { path, .. }
            | Self::Submodule { path }
            | Self::Unreadable { path }
            | Self::InvalidPointer { path, .. }
            | Self::RepoMissing { path, .. }
            | Self::CommitMissing { path, .. }
            | Self::Cycle { path }
            | Self::TooManyHops { path } => Fields::at(path),
            Self::Git(error) => error.fields(),
        }
    }
}
