//! Resolving a logical path to a Location (docs/architecture.md, Pointers:
//! links and mounts). Pointers are not followed yet.

use board_model::{Board, Code, DISPLAY_NAME, Location, LogicalPath, Object, Source};

use crate::address::{self, Named};
use crate::git::{GitError, Repository};
use crate::tree::{Kind, Tree};

impl Repository {
    /// Resolves `path` in `source`.
    pub fn resolve(&self, source: &Source, path: &LogicalPath) -> Result<Location, ResolveError> {
        match source {
            Source::Commit(commit) => self.resolve_in(&mut self.commit_tree(*commit)?, path),
            Source::WorkingTree => self.resolve_in(&mut self.working_tree()?, path),
        }
    }

    fn resolve_in<T: Tree>(
        &self,
        tree: &mut T,
        path: &LogicalPath,
    ) -> Result<Location, ResolveError> {
        let not_found = || ResolveError::NotFound { path: path.clone() };
        let mut object = Object::Directory;
        let mut directory = Some(tree.root());
        let mut physical: Vec<String> = Vec::new();
        let mut segments = path.segments().peekable();
        while let Some(segment) = segments.next() {
            let entries = tree.list(directory.as_ref().ok_or_else(not_found)?)?;
            let entry = match address::named(&entries, segment) {
                Named::Nothing => return Err(not_found()),
                Named::One(entry) => entry,
                Named::RecordWithChildren { record, children } => {
                    if segments.peek().is_some() {
                        children
                    } else {
                        record
                    }
                }
                Named::Shadowed => {
                    return Err(ResolveError::Shadowed {
                        path: path.clone(),
                        segment: segment.to_owned(),
                    });
                }
            };
            physical.push(String::from_utf8_lossy(&entry.name).into_owned());
            (object, directory) = match &entry.kind {
                Kind::Directory(child) => (Object::Directory, Some(child.clone())),
                Kind::File(id) | Kind::Symlink(id) => (Object::Blob(*id), None),
                Kind::Submodule => {
                    return Err(ResolveError::Submodule { path: path.clone() });
                }
                Kind::Unreadable => {
                    return Err(ResolveError::Unreadable { path: path.clone() });
                }
            };
        }
        Ok(Location {
            board: Board {
                repository: self.path().to_path_buf(),
                source: tree.source(),
            },
            path: physical.join("/"),
            object,
        })
    }
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
            Self::Git(error) => error.code(),
        }
    }
}
