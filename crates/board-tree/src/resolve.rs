//! Resolving a logical path to a Location (docs/architecture/paths.md, Pointers:
//! links and mounts). Pointers are followed with POSIX symlink semantics.

use std::collections::{HashSet, VecDeque};
use std::rc::Rc;

use board_model::{
    Board, Code, Fields, Location, LogicalPath, Mount, Object, ObjectId, Pointer, PointerError,
    Source,
};

use crate::address::{Named, Pointers};
use crate::git::{GitError, Repository};
use crate::tree::{Kind, RawEntry, Tree};
use crate::worktree::{Known, WorkingTree};

/// The most pointers one resolution follows, as POSIX allows symlinks.
pub const MAX_HOPS: usize = 40;

/// Where a path that names nothing would be: its deepest ancestor that
/// resolves, and the segments past it (docs/architecture/paths.md, Writes).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ancestor {
    /// Where the ancestor resolved, with every pointer followed.
    pub location: Location,
    /// The segments of the path past the ancestor; at least one.
    pub rest: Vec<String>,
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

impl Repository {
    /// Resolves `path` in `source`, following every pointer on the way,
    /// including one at the end.
    pub fn resolve(&self, source: &Source, path: &LogicalPath) -> Result<Location, ResolveError> {
        match source {
            Source::Commit(commit) => self.resolve_in(
                &mut self.commit_tree(*commit)?,
                path,
                true,
                &mut Pointers::default(),
            ),
            Source::WorkingTree => self.snapshot()?.resolve(path),
        }
    }

    /// The deepest ancestor of `path` that resolves in the working tree,
    /// as [`Resolver::ancestor`] finds it.
    pub fn ancestor(&self, path: &LogicalPath) -> Result<Ancestor, ResolveError> {
        self.snapshot()?.ancestor(path)
    }

    /// A resolver for many paths in one snapshot of the working tree, which
    /// hashes a file only when `known` does not already know its blob.
    pub fn resolver<'k>(&self, known: Known<'k>) -> Result<Resolver<'_, 'k>, GitError> {
        Ok(Resolver {
            repository: self,
            tree: self.working_tree(Some(known))?,
            pointers: Pointers::default(),
        })
    }

    /// A resolver for many paths in one snapshot of the working tree, which
    /// hashes each file it lists. A batch of writes checks every record
    /// against one snapshot, so it lists each directory once.
    pub fn snapshot(&self) -> Result<Resolver<'_, 'static>, GitError> {
        Ok(Resolver {
            repository: self,
            tree: self.working_tree(None)?,
            pointers: Pointers::default(),
        })
    }

    pub(crate) fn resolve_in<T: Tree>(
        &self,
        tree: &mut T,
        path: &LogicalPath,
        follow_last: bool,
        pointers: &mut Pointers,
    ) -> Result<Location, ResolveError> {
        let mut trail = Trail {
            path: path.clone(),
            pointers: Vec::new(),
            visited: HashSet::new(),
            link: None,
            follow_last,
        };
        let segments = path.segments().map(str::to_owned).collect();
        self.follow(tree, segments, 0, &mut trail, pointers)
    }

    /// Resolves `remaining` from the root of `tree`, which is this
    /// repository's, recording on `trail` every pointer followed. A mount
    /// continues in the mounted repository with what remains. The first
    /// `from_link` segments came from the last link's target, so a miss
    /// among them is that link dangling.
    fn follow<T: Tree>(
        &self,
        tree: &mut T,
        mut remaining: VecDeque<String>,
        mut from_link: usize,
        trail: &mut Trail,
        pointers: &mut Pointers,
    ) -> Result<Location, ResolveError> {
        let path = &trail.path.clone();
        let board = Board {
            repository: self.path().to_path_buf(),
            source: tree.source(),
        };
        let mut directory = Some(tree.root());
        let mut physical: Vec<String> = Vec::new();
        let mut object = Object::Directory;
        while let Some(segment) = remaining.pop_front() {
            let missing = |from_link: usize, trail: &Trail| match &trail.link {
                Some((link, target)) if from_link > 0 => ResolveError::Dangling {
                    path: path.clone(),
                    link: link.clone(),
                    target: target.clone(),
                },
                _ => ResolveError::NotFound { path: path.clone() },
            };
            let Some(current) = directory.take() else {
                return Err(missing(from_link, trail));
            };
            let entries = tree.list(&current)?;
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
            from_link = usize::saturating_sub(from_link, 1);
            physical.push(String::from_utf8_lossy(&entry.name).into_owned());
            match &entry.kind {
                Kind::Directory(child) => {
                    object = Object::Directory;
                    directory = Some(child.clone());
                }
                Kind::File(id) => {
                    object = Object::Blob(*id);
                    if remaining.is_empty() && !trail.follow_last {
                        continue;
                    }
                    let Some(next) = pointer(tree, &current, entry, path, pointers)? else {
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
                            directory = Some(tree.root());
                            physical.clear();
                            object = Object::Directory;
                        }
                        Pointer::Mount(mount) => {
                            return self.enter(&mount, remaining, from_link, trail, pointers);
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
            }
        }
        Ok(Location {
            board,
            path: physical.join("/"),
            object,
            pointers: std::mem::take(&mut trail.pointers),
        })
    }

    /// The repository `mount` names, read against this one, or its home for
    /// `~/`.
    fn mounted_repository(&self, mount: &Mount) -> Result<Repository, MountMissing> {
        let location = match mount.repo.strip_prefix("~/") {
            Some(rest) => self.home().ok_or(MountMissing::Repo)?.join(rest),
            None => self.path().join(&mount.repo),
        };
        let mut mounted = Repository::open(&location).map_err(|_| MountMissing::Repo)?;
        if let Some(home) = self.home() {
            mounted = mounted.with_home(home.to_path_buf());
        }
        Ok(mounted)
    }

    /// The repository `mount` names and the root tree of its commit.
    fn open_mount(&self, mount: &Mount) -> Result<(Repository, ObjectId), MountMissing> {
        let mounted = self.mounted_repository(mount)?;
        let root = mounted
            .commit_tree(mount.commit)
            .map_err(|_| MountMissing::Commit)?
            .root();
        Ok((mounted, root))
    }

    /// The commit the mounted repository's `follow` ref names now. A
    /// missing repository is `mount::repo_missing`; a ref that is missing,
    /// not a valid name, or not peeled to a commit is `mount::follow_missing`.
    pub(crate) fn followed(&self, mount: &Mount) -> Result<ObjectId, Code> {
        let mounted = self
            .mounted_repository(mount)
            .map_err(|_| Code::MountRepoMissing)?;
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

    /// The repository `mount` names and the root tree of its commit, or the
    /// code that says why it cannot be read.
    pub(crate) fn mounted(&self, mount: &Mount) -> Result<(Repository, ObjectId), Code> {
        self.open_mount(mount).map_err(|missing| match missing {
            MountMissing::Repo => Code::MountRepoMissing,
            MountMissing::Commit => Code::MountCommitMissing,
        })
    }

    /// Continues with `remaining` at the root of the tree `mount` names.
    fn enter(
        &self,
        mount: &Mount,
        remaining: VecDeque<String>,
        from_link: usize,
        trail: &mut Trail,
        pointers: &mut Pointers,
    ) -> Result<Location, ResolveError> {
        let at = trail.last_pointer();
        let (mounted, _) = self.open_mount(mount).map_err(|missing| match missing {
            MountMissing::Repo => ResolveError::RepoMissing {
                path: at.clone(),
                repo: mount.repo.clone(),
            },
            MountMissing::Commit => ResolveError::CommitMissing {
                path: at.clone(),
                commit: mount.commit,
            },
        })?;
        let mut tree =
            mounted
                .commit_tree(mount.commit)
                .map_err(|_| ResolveError::CommitMissing {
                    path: at,
                    commit: mount.commit,
                })?;
        let board = Board {
            repository: mounted.path().to_path_buf(),
            source: tree.source(),
        };
        // Mounts alone cannot cycle: each pins a commit, and no commit can
        // hold its own id. A cycle needs a link, and the hop limit bounds
        // the rest; this key catches a link cycle met through a mount.
        if !trail.visited.insert((board, remaining.clone())) {
            return Err(ResolveError::Cycle {
                path: trail.path.clone(),
            });
        }
        mounted.follow(&mut tree, remaining, from_link, trail, pointers)
    }
}

/// Resolves paths in one snapshot of the working tree: each directory is
/// listed once, however many paths pass through it.
pub struct Resolver<'r, 'k> {
    repository: &'r Repository,
    tree: WorkingTree<'r, 'k>,
    pointers: Pointers,
}

impl std::fmt::Debug for Resolver<'_, '_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Resolver")
            .field("repository", &self.repository.path())
            .finish_non_exhaustive()
    }
}

impl Resolver<'_, '_> {
    /// Resolves `path` as [`Repository::resolve`] does in the working tree.
    pub fn resolve(&mut self, path: &LogicalPath) -> Result<Location, ResolveError> {
        self.repository
            .resolve_in(&mut self.tree, path, true, &mut self.pointers)
    }

    /// Resolves `path` as [`Resolver::resolve`] does, except that a pointer
    /// at the end is not followed, as `lstat` does not follow a symlink.
    pub fn resolve_pointer(&mut self, path: &LogicalPath) -> Result<Location, ResolveError> {
        self.repository
            .resolve_in(&mut self.tree, path, false, &mut self.pointers)
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
            // A prefix of a logical path's segments is always one.
            let Ok(ancestor) = format!("/{}", segments[..depth].join("/")).parse::<LogicalPath>()
            else {
                break;
            };
            match self.resolve(&ancestor) {
                Ok(location) => {
                    return Ok(Ancestor {
                        location,
                        rest: segments[depth..]
                            .iter()
                            .map(|segment| (*segment).to_owned())
                            .collect(),
                    });
                }
                Err(ResolveError::NotFound { .. }) => {}
                Err(error) => return Err(error),
            }
        }
        Err(ResolveError::NotFound { path: path.clone() })
    }
}

/// What a mount names that cannot be read.
enum MountMissing {
    Repo,
    Commit,
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

/// The entry `segment` names in `entries`, the listing of `directory`, or
/// `None` when it names nothing. A record beside its directory of children
/// is the record when `last`, else the directory, unless it shadows the
/// directory.
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

/// The pointer `entry` holds, if it is a `.json` file whose record is one.
fn pointer<T: Tree>(
    tree: &mut T,
    directory: &T::Directory,
    entry: &RawEntry<T::Directory>,
    path: &LogicalPath,
    pointers: &mut Pointers,
) -> Result<Option<Pointer>, ResolveError> {
    if !matches!(entry.kind, Kind::File(_)) || !entry.name.ends_with(b".json") {
        return Ok(None);
    }
    let parsed = pointers.parse(entry, || tree.read(directory, entry))?;
    parsed.map_err(|source| ResolveError::InvalidPointer {
        path: path.clone(),
        pointer: String::from_utf8_lossy(&entry.name).into_owned(),
        source,
    })
}

/// A path that does not resolve.
#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error("nothing is at {path}")]
    NotFound { path: LogicalPath },
    #[error("{segment:?} in {path} names more than one entry")]
    Shadowed { path: LogicalPath, segment: String },
    #[error("{path} reaches a submodule, which is not descended")]
    Submodule { path: LogicalPath },
    #[error("{path} reaches a file that cannot be read")]
    Unreadable { path: LogicalPath },
    #[error("the link at {link} points to {target}, which is missing")]
    Dangling {
        path: LogicalPath,
        link: LogicalPath,
        target: LogicalPath,
    },
    #[error("{path} reaches the invalid pointer {pointer:?}: {source}")]
    InvalidPointer {
        path: LogicalPath,
        pointer: String,
        #[source]
        source: Rc<PointerError>,
    },
    #[error("{path} mounts the repository {repo:?}, which cannot be opened")]
    RepoMissing { path: LogicalPath, repo: String },
    #[error("{path} mounts commit {commit}, which its repository does not hold")]
    CommitMissing { path: LogicalPath, commit: ObjectId },
    #[error("{path} returns to a board and path it already resolved through a pointer")]
    Cycle { path: LogicalPath },
    #[error("{path} follows more than {MAX_HOPS} pointers")]
    TooManyHops { path: LogicalPath },
    #[error(transparent)]
    Git(#[from] GitError),
}

impl ResolveError {
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
            Self::Git(error) => error.code(),
        }
    }

    /// The path at fault: the link for a dangling link, else the path.
    pub fn fields(&self) -> Fields {
        match self {
            Self::Dangling { link, .. } => Fields::at(link),
            Self::NotFound { path }
            | Self::Shadowed { path, .. }
            | Self::Submodule { path }
            | Self::Unreadable { path }
            | Self::InvalidPointer { path, .. }
            | Self::RepoMissing { path, .. }
            | Self::CommitMissing { path, .. }
            | Self::Cycle { path }
            | Self::TooManyHops { path } => Fields::at(path),
            Self::Git(_) => Fields::default(),
        }
    }
}
