//! `board init`: making a git repository's working tree a board
//! (docs/architecture.md, Boards and entries).

use std::path::{Path, PathBuf};

use board_model::{CONFIG_TYPE, Code, LogicalPath, Source, canonical_json};
use board_tree::{GitError, LOCK_WAIT, Repositories, WriteError, repository_root};
use serde_json::json;

use crate::board::read_config;

/// The board `init` made, or found already made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InitReport {
    pub root: PathBuf,
    /// Whether `init` wrote `.board/board.json`; false when one was there.
    pub created: bool,
}

/// Makes the working tree of the git repository holding `start` a board, by
/// writing a minimal `.board/board.json` at its root, through the resolver
/// as `put` writes. A valid one is left as it is, and anything else at that
/// path is refused. Nothing is committed.
pub fn init(start: &Path) -> Result<InitReport, InitError> {
    let root = repository_root(start).ok_or_else(|| InitError::NotARepository {
        start: start.to_path_buf(),
    })?;
    let repositories =
        Repositories::open(&root, None).map_err(|source| InitError::Git { source })?;
    let repository = repositories.root();
    if repository.path() != root {
        return Err(InitError::NotARepository {
            start: start.to_path_buf(),
        });
    }
    let write = |source| InitError::Write { source };
    let lock = repository.lock(LOCK_WAIT).map_err(write)?;
    let invalid = || InitError::Config { root: root.clone() };
    let path: LogicalPath = "/.board/board".parse().map_err(|_| invalid())?;
    if repositories.resolve(&Source::WorkingTree, &path).is_ok() {
        read_config(&repositories).ok_or_else(invalid)?;
        return Ok(InitReport {
            root,
            created: false,
        });
    }
    let target = repositories.target(&path, false).map_err(write)?;
    let config = canonical_json(&json!({ "$type": CONFIG_TYPE })).into_bytes();
    repository
        .write_file(&lock, &target.path, &config)
        .map_err(write)?;
    Ok(InitReport {
        root,
        created: true,
    })
}

#[derive(Debug, thiserror::Error)]
pub enum InitError {
    #[error("no git repository with a working tree holds {}; run `git init` there first", start.display())]
    NotARepository { start: PathBuf },
    #[error("{} has a .board/board.json that is not a valid board/config/v1", root.display())]
    Config { root: PathBuf },
    #[error(transparent)]
    Git { source: GitError },
    #[error(transparent)]
    Write { source: WriteError },
}

impl InitError {
    pub fn code(&self) -> Code {
        match self {
            Self::NotARepository { .. } => Code::ConfigNotFound,
            Self::Config { .. } => Code::ConfigInvalid,
            Self::Git { source } => source.code(),
            Self::Write { source } => source.code(),
        }
    }
}

#[cfg(test)]
mod tests;
