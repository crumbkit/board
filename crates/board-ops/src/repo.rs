//! `board repo add`: writing a repository record once, at the path its
//! remote's identity gives, and [`ensure`], the put line every writer that
//! names a repository adds to its own batch
//! (docs/architecture/paths/repos.md).

use std::path::{Path, PathBuf};

use board_model::{
    Code, Fields, IdentityError, LogicalPath, PathError, Refusal, Remote, RepositoryRecord,
    canonical_json, origin_identity, repo_identity,
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

/// The line of a `put --batch` that writes the record of `remote` in the
/// board at `within`, `/` for `board` itself or the path of a board it
/// mounts, when that board holds no record there yet. The line expects
/// nothing at the path, so a writer that got there first makes the batch
/// stale. It is `None` when the board already holds the record. A writer
/// that names a repository adds this line to the batch that writes its own
/// record, giving `within` as the board that batch writes in.
pub fn ensure(
    board: &Board,
    within: &LogicalPath,
    remote: &Remote,
) -> Result<Option<BatchLine<Value>>, EnsureError> {
    let path = within
        .beneath(remote.path())
        .map_err(|source| EnsureError::Path { source })?;
    Ok(
        (!present(board, &path, remote)?).then(|| BatchLine::Record {
            path: path.to_string(),
            expect: Some("none".to_owned()),
            record: RepositoryRecord::new(remote.clone()).to_value(),
        }),
    )
}

/// Whether `path` in `board` holds the valid record of `remote`. A path
/// that holds anything else is refused, so a hand-edited record is never
/// taken for the right one.
fn present(board: &Board, path: &LogicalPath, remote: &Remote) -> Result<bool, EnsureError> {
    let report = match board.get(path) {
        Ok(report) => report,
        Err(error) if error.code() == Code::PathNotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    let occupied = |code, why: String| EnsureError::Occupied {
        path: report.display.clone(),
        remote: remote.clone(),
        code,
        why,
    };
    let value = (report.record.as_ref())
        .map_err(|refusal| occupied(Code::RepoInvalid, refusal.message.clone()))?;
    let record = RepositoryRecord::from_value(value)
        .map_err(|error| occupied(Code::RepoInvalid, error.to_string()))?;
    if record.remote == *remote {
        Ok(true)
    } else {
        Err(occupied(
            Code::RepoPathMismatch,
            format!("it names the repository {}", record.remote),
        ))
    }
}

/// Why [`ensure`] gave no line.
#[derive(Debug, thiserror::Error)]
pub enum EnsureError {
    /// Something other than the record of `remote` is at its path.
    #[error("{path} holds no valid record of {remote}: {why}")]
    Occupied {
        path: LogicalPath,
        remote: Remote,
        code: Code,
        why: String,
    },
    /// The record's path beneath the mount is not a board path.
    #[error("the record's path beneath the mount is not a board path: {source}")]
    Path { source: PathError },
    #[error(transparent)]
    Read(#[from] ReadError),
}

impl EnsureError {
    pub fn code(&self) -> Code {
        match self {
            Self::Occupied { code, .. } => *code,
            Self::Path { .. } => Code::RepoTooLong,
            Self::Read(error) => error.code(),
        }
    }

    pub fn refusal(&self) -> Refusal {
        match self {
            Self::Read(error) => error.refusal("repo add"),
            Self::Occupied { path, .. } => Refusal::new(self.code(), self, &Fields::default())
                .with_hint(format!(
                    "look at the file with `board get {path}`; if it is not needed, remove it with `board rm {path}` and run repo add again"
                )),
            Self::Path { .. } => Refusal::new(self.code(), self, &Fields::default()),
        }
    }
}

impl Board {
    /// Writes the record of the repository `remote` names, a remote in any
    /// form [`repo_identity`] reads, at `/repositories/<remote>`, through
    /// put with `--expect none`; or writes nothing when the record is
    /// already there.
    pub fn repo_add(&self, remote: &str) -> Result<RepoAdded, RepoError> {
        let identity = repo_identity(remote).map_err(|source| RepoError::Remote {
            source,
            origin: false,
        })?;
        self.repo_write(identity)
    }

    /// [`Board::repo_add`] of the `origin` remote of the git repository
    /// that holds `directory`, which [`origin_identity`] reads, so a
    /// relative origin is a local path.
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
        let identity = origin_identity(&url).map_err(|source| RepoError::Remote {
            source,
            origin: true,
        })?;
        self.repo_write(identity)
    }

    fn repo_write(&self, remote: Remote) -> Result<RepoAdded, RepoError> {
        if present(self, remote.path(), &remote)? {
            return Ok(RepoAdded::Unchanged { remote });
        }
        let record = canonical_json(&RepositoryRecord::new(remote.clone()).to_value());
        let report = self.put(remote.path(), Expect::Absent, record.as_bytes())?;
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
    Ensure(#[from] EnsureError),
    #[error(transparent)]
    Put(#[from] PutError),
}

impl RepoError {
    pub fn code(&self) -> Code {
        match self {
            Self::Remote { source, .. } => source.code(),
            Self::NoRemote { .. } => Code::MountNoRemote,
            Self::Outside { .. } => Code::MountRepoMissing,
            Self::Ensure(error) => error.code(),
            Self::Put(error) => error.code(),
        }
    }

    pub fn refusal(&self) -> Refusal {
        match self {
            Self::Ensure(error) => error.refusal(),
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
