//! Why a path does not resolve.

use std::sync::Arc;

use board_model::{Code, Fields, LogicalPath, ObjectId, PointerError};

use super::MAX_HOPS;
use crate::git::GitError;
use crate::symlink::Refused;

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

    /// Whether resolving stopped because `path` is itself a mount whose
    /// repository cannot be opened or lacks the pinned commit.
    pub fn is_unread_mount(&self, path: &LogicalPath) -> bool {
        matches!(
            self,
            Self::RepoMissing { path: at, .. } | Self::CommitMissing { path: at, .. } if at == path
        )
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
