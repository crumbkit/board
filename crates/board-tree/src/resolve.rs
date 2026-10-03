//! Resolving a logical path to a Location (docs/architecture.md, Pointers:
//! links and mounts). Pointers are not followed yet.

use board_model::{Board, Code, Location, LogicalPath, Object, ObjectId, Source};

use crate::address;
use crate::git::{GitError, Kind, Repository};

impl Repository {
    /// Resolves `path` in the tree of `commit`.
    pub fn resolve(&self, commit: ObjectId, path: &LogicalPath) -> Result<Location, ResolveError> {
        let mut object = Object::Directory;
        let mut tree = Some(self.root_tree(commit)?);
        let mut physical: Vec<String> = Vec::new();
        for segment in path.segments() {
            let not_found = || ResolveError::NotFound { path: path.clone() };
            let entries = self.list(tree.ok_or_else(not_found)?)?;
            let mut matches = entries
                .iter()
                .filter(|entry| address::segment(entry) == Some(segment));
            let entry = matches.next().ok_or_else(not_found)?;
            if matches.next().is_some() {
                return Err(ResolveError::Shadowed {
                    path: path.clone(),
                    segment: segment.to_owned(),
                });
            }
            physical.push(String::from_utf8_lossy(&entry.name).into_owned());
            (object, tree) = match entry.kind {
                Kind::Directory => (Object::Directory, Some(entry.id)),
                Kind::File | Kind::Symlink | Kind::Submodule => (Object::Blob(entry.id), None),
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
    #[error(transparent)]
    Git(#[from] GitError),
}

impl ResolveError {
    pub fn code(&self) -> Code {
        match self {
            Self::NotFound { .. } => Code::PathNotFound,
            Self::Shadowed { .. } => Code::PathShadowed,
            Self::Git(error) => error.code(),
        }
    }
}
