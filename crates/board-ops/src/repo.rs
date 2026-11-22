//! `board repo add`: writing a repository record once, at the path its
//! remote's identity gives, and [`ensure`], the put line every writer that
//! names a repository adds to its own batch
//! (docs/architecture/paths/repos.md).

use std::path::{Path, PathBuf};

use board_model::{
    Code, Fields, IdentityError, Refusal, Remote, RepositoryRecord, canonical_json, repo_identity,
};
use board_tree::Origin;
use board_types::BatchLine;
use serde_json::Value;

use crate::board::Board;
use crate::put::{Expect, PutError, PutReport};
use crate::read::ReadError;

/// What `repo add` did.
#[derive(Clone, Debug, PartialEq)]
pub enum RepoAdded {
    /// It wrote the record of `remote`, with put's report.
    Wrote { remote: Remote, report: PutReport },
    /// Something was already at the path of `remote`'s record, so it wrote
    /// nothing.
    Unchanged { remote: Remote },
}

impl RepoAdded {
    /// The repository whose record the run wrote or found.
    pub fn remote(&self) -> &Remote {
        match self {
            Self::Wrote { remote, .. } | Self::Unchanged { remote } => remote,
        }
    }
}

/// The line of a `put --batch` that writes the record of `remote` in
/// `board` when nothing is at its path yet, expecting nothing there so a
/// writer that got there first makes the batch stale; `None` when
/// something is there. A writer that names a repository adds this line to
/// the batch that writes its own record.
pub fn ensure(board: &Board, remote: &Remote) -> Result<Option<BatchLine<Value>>, ReadError> {
    match board.get(remote.path()) {
        Ok(_) => Ok(None),
        Err(error) if error.code() == Code::PathNotFound => Ok(Some(BatchLine::Record {
            path: remote.path().to_string(),
            expect: Some("none".to_owned()),
            record: RepositoryRecord::new(remote.clone()).to_value(),
        })),
        Err(error) => Err(error),
    }
}

impl Board {
    /// Writes the record of the repository `remote` names, a remote in any
    /// form [`repo_identity`] reads, at `/repositories/<remote>`, through
    /// put with `--expect none`; or writes nothing when something is
    /// already there.
    pub fn repo_add(&self, remote: &str) -> Result<RepoAdded, RepoError> {
        let identity = repo_identity(remote).map_err(|source| RepoError::Remote {
            source,
            origin: false,
        })?;
        self.repo_write(identity)
    }

    /// [`Board::repo_add`] of the `origin` remote of the git repository
    /// that holds `directory`.
    pub fn repo_add_from(&self, directory: &Path) -> Result<RepoAdded, RepoError> {
        let url = match board_tree::origin(directory) {
            Origin::Url(url) => url,
            Origin::Outside => {
                return Err(RepoError::Outside {
                    directory: directory.to_path_buf(),
                });
            }
            Origin::Unread(why) => {
                return Err(RepoError::NoRemote {
                    directory: directory.to_path_buf(),
                    why,
                });
            }
        };
        let identity = repo_identity(&url).map_err(|source| RepoError::Remote {
            source,
            origin: true,
        })?;
        self.repo_write(identity)
    }

    fn repo_write(&self, remote: Remote) -> Result<RepoAdded, RepoError> {
        let Some(BatchLine::Record { record, .. }) = ensure(self, &remote)? else {
            return Ok(RepoAdded::Unchanged { remote });
        };
        let bytes = canonical_json(&record);
        let report = self.put(remote.path(), Expect::Absent, bytes.as_bytes())?;
        Ok(RepoAdded::Wrote { remote, report })
    }
}

/// Why `repo add` wrote nothing.
#[derive(Debug, thiserror::Error)]
pub enum RepoError {
    /// The remote, or the clone's origin when `origin`, has no identity.
    #[error("the {} {source}", if *origin { "origin" } else { "remote" })]
    Remote { source: IdentityError, origin: bool },
    #[error("{} has no origin remote: {why}", directory.display())]
    NoRemote { directory: PathBuf, why: String },
    #[error("{} is in no git repository", directory.display())]
    Outside { directory: PathBuf },
    #[error(transparent)]
    Read(#[from] ReadError),
    #[error(transparent)]
    Put(#[from] PutError),
}

impl RepoError {
    pub fn code(&self) -> Code {
        match self {
            Self::Remote { source, .. } => source.code(),
            Self::NoRemote { .. } => Code::MountNoRemote,
            Self::Outside { .. } => Code::MountRepoMissing,
            Self::Read(error) => error.code(),
            Self::Put(error) => error.code(),
        }
    }

    pub fn refusal(&self) -> Refusal {
        match self {
            Self::Read(error) => error.refusal("repo add"),
            Self::Put(error) => error.refusal(),
            Self::Outside { .. } => Refusal::new(self.code(), self, &Fields::default()).with_hint(
                "give --from the directory of a git clone, or give repo add the remote itself",
            ),
            _ => Refusal::new(self.code(), self, &Fields::default()),
        }
    }
}

#[cfg(test)]
mod tests;
