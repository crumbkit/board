//! Finding and opening a board (docs/architecture.md, Boards and entries).

use std::path::{Path, PathBuf};

use board_model::{CONFIG_TYPE, Code, Kind, LogicalPath, Object, RecordType, Source, classify};
use board_tree::{GitError, Repository};
use serde::Deserialize;
use serde::de::IgnoredAny;
use serde_json::Value;

/// `.board/board.json`, a `board/config/v1` record.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(rename = "$type")]
    _tag: IgnoredAny,
    #[serde(default = "commits_on_put")]
    pub commit_on_put: bool,
    #[serde(default)]
    pub plugins: Vec<String>,
}

fn commits_on_put() -> bool {
    true
}

impl Config {
    /// The configuration a parsed `board/config/v1` record holds.
    pub(crate) fn from_value(value: &Value) -> Option<Self> {
        Self::deserialize(value).ok()
    }
}

/// An open board: the working tree of its repository, and its
/// configuration.
#[derive(Debug)]
pub struct Board {
    repository: Repository,
    config: Config,
}

impl Board {
    /// Opens the board whose root is `start` or the nearest directory above
    /// it holding `.board/board.json`. That directory must be the root of its
    /// repository's working tree. `home` is what a mount's `~/` means.
    pub fn open(start: &Path, home: Option<PathBuf>) -> Result<Self, OpenError> {
        let not_found = || OpenError::NotFound {
            start: start.to_path_buf(),
        };
        let root = board_tree::discover(start).ok_or_else(not_found)?;
        if !board_tree::is_repository_root(&root) {
            return Err(not_found());
        }
        let mut repository = Repository::open(&root).map_err(|source| OpenError::Git { source })?;
        if repository.path() != root {
            return Err(not_found());
        }
        if let Some(home) = home {
            repository = repository.with_home(home);
        }
        let config = read_config(&repository).ok_or(OpenError::Config { root })?;
        Ok(Self { repository, config })
    }

    pub fn repository(&self) -> &Repository {
        &self.repository
    }

    pub fn config(&self) -> &Config {
        &self.config
    }
}

/// The configuration in the working tree's `.board/board.json`.
fn read_config(repository: &Repository) -> Option<Config> {
    let path: LogicalPath = "/.board/board".parse().ok()?;
    let location = repository.resolve(&Source::WorkingTree, &path).ok()?;
    let Object::Blob(blob) = location.object else {
        return None;
    };
    let bytes = repository
        .read(&Source::WorkingTree, &location.path, blob)
        .ok()?;
    let classified = classify(&location.path, bytes.as_deref());
    let is_config = classified.kind == Kind::Record
        && classified.code.is_none()
        && classified.record_type.as_ref().map(RecordType::as_str) == Some(CONFIG_TYPE);
    if !is_config {
        return None;
    }
    Config::from_value(classified.value.as_ref()?)
}

/// A board that cannot be opened.
#[derive(Debug, thiserror::Error)]
pub enum OpenError {
    #[error("no board at or above {start:?}: no .board/board.json at a repository root")]
    NotFound { start: PathBuf },
    #[error("the board at {root:?} has a .board/board.json that is not a valid board/config/v1")]
    Config { root: PathBuf },
    #[error(transparent)]
    Git { source: GitError },
}

impl OpenError {
    pub fn code(&self) -> Code {
        match self {
            Self::NotFound { .. } => Code::ConfigNotFound,
            Self::Config { .. } => Code::ConfigInvalid,
            Self::Git { source } => source.code(),
        }
    }
}
