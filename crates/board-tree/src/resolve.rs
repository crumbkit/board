//! Resolving a logical path to a Location (docs/architecture.md, Pointers:
//! links and mounts). Links are followed with POSIX symlink semantics.

use std::collections::{HashSet, VecDeque};

use board_model::{
    Board, Code, DISPLAY_NAME, Location, LogicalPath, Object, Pointer, PointerError, Source,
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
            Source::WorkingTree => self.resolve_in(&mut self.working_tree()?, path),
        }
    }

    pub(crate) fn resolve_in<T: Tree>(
        &self,
        tree: &mut T,
        path: &LogicalPath,
    ) -> Result<Location, ResolveError> {
        let board = Board {
            repository: self.path().to_path_buf(),
            source: tree.source(),
        };
        let mut remaining: VecDeque<String> = path.segments().map(str::to_owned).collect();
        let mut directory = Some(tree.root());
        let mut physical: Vec<String> = Vec::new();
        let mut object = Object::Directory;
        let mut pointers: Vec<Location> = Vec::new();
        let mut visited: HashSet<VecDeque<String>> = HashSet::new();
        // Segments still to read that came from the last link's target, so
        // that a miss among them is the link dangling.
        let mut from_link = 0;
        while let Some(segment) = remaining.pop_front() {
            let missing = |from_link: usize, pointers: &[Location]| match pointers.last() {
                Some(link) if from_link > 0 => ResolveError::Dangling {
                    path: path.clone(),
                    link: link.path.clone(),
                },
                _ => ResolveError::NotFound { path: path.clone() },
            };
            let Some(current) = directory.take() else {
                return Err(missing(from_link, &pointers));
            };
            let entries = tree.list(&current)?;
            let entry = match address::named(&entries, &segment) {
                Named::Nothing => return Err(missing(from_link, &pointers)),
                Named::One(entry) => entry,
                Named::RecordWithChildren { record, children } => {
                    if pointer(tree, &current, record, path)?.is_some() {
                        return Err(ResolveError::Shadowed {
                            path: path.clone(),
                            segment,
                        });
                    }
                    if remaining.is_empty() {
                        record
                    } else {
                        children
                    }
                }
                Named::Shadowed => {
                    return Err(ResolveError::Shadowed {
                        path: path.clone(),
                        segment,
                    });
                }
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
                    if let Some(Pointer::Link(target)) = pointer(tree, &current, entry, path)? {
                        pointers.push(Location {
                            board: board.clone(),
                            path: physical.join("/"),
                            object,
                            pointers: Vec::new(),
                        });
                        if pointers.len() > MAX_HOPS {
                            return Err(ResolveError::TooManyHops { path: path.clone() });
                        }
                        let next: VecDeque<String> = target
                            .segments()
                            .map(str::to_owned)
                            .chain(remaining.drain(..))
                            .collect();
                        if !visited.insert(next.clone()) {
                            return Err(ResolveError::Cycle { path: path.clone() });
                        }
                        from_link = target.segments().count();
                        remaining = next;
                        directory = Some(tree.root());
                        physical.clear();
                        object = Object::Directory;
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
            pointers,
        })
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
    #[error("{path} reaches a submodule, which {} does not descend", DISPLAY_NAME)]
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
    #[error("{path} returns to a path it already resolved through a link")]
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
            Self::Cycle { .. } => Code::ResolveCycle,
            Self::TooManyHops { .. } => Code::ResolveTooManyHops,
            Self::Git(error) => error.code(),
        }
    }
}
