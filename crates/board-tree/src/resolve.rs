//! Resolving a logical path to a Location (docs/architecture.md, Pointers:
//! links and mounts). Pointers are not followed yet.

use board_model::{Board, Code, Location, LogicalPath, Object, ObjectId, Source};

use crate::address::{self, Named};
use crate::git::{GitError, Kind, Repository};

impl Repository {
    /// Resolves `path` in the tree of `commit`.
    pub fn resolve(&self, commit: ObjectId, path: &LogicalPath) -> Result<Location, ResolveError> {
        let mut object = Object::Directory;
        let mut tree = Some(self.root_tree(commit)?);
        let mut physical: Vec<String> = Vec::new();
        let mut segments = path.segments().peekable();
        while let Some(segment) = segments.next() {
            let not_found = || ResolveError::NotFound { path: path.clone() };
            let entries = self.list(tree.ok_or_else(not_found)?)?;
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
            (object, tree) = match entry.kind {
                Kind::Directory => (Object::Directory, Some(entry.id)),
                Kind::File | Kind::Symlink => (Object::Blob(entry.id), None),
                Kind::Submodule => {
                    return Err(ResolveError::Submodule { path: path.clone() });
                }
            };
        }
        Ok(Location {
            board: Board {
                repository: self.path().to_path_buf(),
                source: Source::Commit(commit),
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
    #[error("{path} reaches a submodule, which Board does not descend")]
    Submodule { path: LogicalPath },
    #[error(transparent)]
    Git(#[from] GitError),
}

impl ResolveError {
    pub fn code(&self) -> Code {
        match self {
            Self::NotFound { .. } => Code::PathNotFound,
            Self::Shadowed { .. } => Code::PathShadowed,
            Self::Submodule { .. } => Code::RepoSubmodule,
            Self::Git(error) => error.code(),
        }
    }
}
