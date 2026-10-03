//! Resolving a logical path to a Location (docs/architecture.md, Pointers:
//! links and mounts). Pointers are followed with POSIX symlink semantics.

use std::collections::{HashSet, VecDeque};

use board_model::{
    Board, Code, Location, LogicalPath, Mount, Object, ObjectId, Pointer, PointerError, Source,
};

use crate::address::{self, Named};
use crate::git::{GitError, Repository};
use crate::tree::{Kind, RawEntry, Tree};

/// The most pointers one resolution follows, as POSIX allows symlinks.
pub const MAX_HOPS: usize = 40;

impl Repository {
    /// Resolves `path` in `source`, following every pointer on the way,
    /// including one at the end.
    pub fn resolve(&self, source: &Source, path: &LogicalPath) -> Result<Location, ResolveError> {
        match source {
            Source::Commit(commit) => self.resolve_in(&mut self.commit_tree(*commit)?, path),
            Source::WorkingTree => self.resolve_in(&mut self.working_tree(&mut |_, _| None)?, path),
        }
    }

    pub(crate) fn resolve_in<T: Tree>(
        &self,
        tree: &mut T,
        path: &LogicalPath,
    ) -> Result<Location, ResolveError> {
        let mut trail = Trail {
            path: path.clone(),
            pointers: Vec::new(),
            visited: HashSet::new(),
            link: None,
        };
        let segments = path.segments().map(str::to_owned).collect();
        self.follow(tree, segments, 0, &mut trail)
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
                Some(link) if from_link > 0 => ResolveError::Dangling {
                    path: path.clone(),
                    link: link.clone(),
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
                    let Some(next) = pointer(tree, &current, entry, path)? else {
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
                            trail.link = Some(physical.join("/"));
                            remaining = next;
                            directory = Some(tree.root());
                            physical.clear();
                            object = Object::Directory;
                        }
                        Pointer::Mount(mount) => {
                            return self.enter(&mount, remaining, from_link, trail);
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

    /// Continues with `remaining` at the root of the tree `mount` names.
    fn enter(
        &self,
        mount: &Mount,
        remaining: VecDeque<String>,
        from_link: usize,
        trail: &mut Trail,
    ) -> Result<Location, ResolveError> {
        let path = trail.path.clone();
        let repo_missing = || ResolveError::RepoMissing {
            path: path.clone(),
            repo: mount.repo.clone(),
        };
        let location = match mount.repo.strip_prefix("~/") {
            Some(rest) => self.home().ok_or_else(repo_missing)?.join(rest),
            None => self.path().join(&mount.repo),
        };
        let mut mounted = Repository::open(&location).map_err(|_| repo_missing())?;
        if let Some(home) = self.home() {
            mounted = mounted.with_home(home.to_path_buf());
        }
        let mut tree =
            mounted
                .commit_tree(mount.commit)
                .map_err(|_| ResolveError::CommitMissing {
                    path: path.clone(),
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
            return Err(ResolveError::Cycle { path });
        }
        mounted.follow(&mut tree, remaining, from_link, trail)
    }
}

/// What one resolution carries across links and mounts.
struct Trail {
    /// The path being resolved, for errors.
    path: LogicalPath,
    pointers: Vec<Location>,
    /// Each board and path resolution restarted at.
    visited: HashSet<(Board, VecDeque<String>)>,
    /// The physical path of the last link followed.
    link: Option<String>,
}

/// The entry `segment` names in `entries`, the listing of `directory`, or
/// `None` when it names nothing. A record beside its directory of children
/// is the record when `last`, else the directory; a pointer there, or any
/// other double match, is shadowed.
fn pick<'e, T: Tree>(
    tree: &mut T,
    directory: &T::Directory,
    entries: &'e [RawEntry<T::Directory>],
    segment: String,
    last: bool,
    path: &LogicalPath,
) -> Result<Option<&'e RawEntry<T::Directory>>, ResolveError> {
    let shadowed = |segment| ResolveError::Shadowed {
        path: path.clone(),
        segment,
    };
    match address::named(entries, &segment) {
        Named::Nothing => Ok(None),
        Named::One(entry) => Ok(Some(entry)),
        Named::RecordWithChildren { record, children } => {
            if pointer(tree, directory, record, path)?.is_some() {
                return Err(shadowed(segment));
            }
            Ok(Some(if last { record } else { children }))
        }
        Named::Shadowed => Err(shadowed(segment)),
    }
}

/// The pointer `entry` holds, if it is a `.json` file whose record is one.
fn pointer<T: Tree>(
    tree: &mut T,
    directory: &T::Directory,
    entry: &RawEntry<T::Directory>,
    path: &LogicalPath,
) -> Result<Option<Pointer>, ResolveError> {
    if !matches!(entry.kind, Kind::File(_)) || !entry.name.ends_with(b".json") {
        return Ok(None);
    }
    let Some(bytes) = tree.read(directory, entry)? else {
        return Ok(None);
    };
    board_model::pointer(&bytes).map_err(|source| ResolveError::InvalidPointer {
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
    #[error("{path} follows a link to {link:?}, whose target is missing")]
    Dangling { path: LogicalPath, link: String },
    #[error("{path} reaches the invalid pointer {pointer:?}: {source}")]
    InvalidPointer {
        path: LogicalPath,
        pointer: String,
        #[source]
        source: PointerError,
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
}
