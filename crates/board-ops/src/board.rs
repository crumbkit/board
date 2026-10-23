//! Finding and opening a board (docs/architecture.md, Boards and entries).

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use board_model::{
    CONFIG_TYPE, Classified, Code, Content, Fields, LogicalPath, Object, ObjectId, Source, classify,
};
use board_plugin::PluginName;
use board_tree::{GitError, Repositories, Repository, ResolveError, latest};
use serde::Deserialize;
use serde::de::IgnoredAny;
use serde_json::Value;

use crate::trace::Trace;

/// `.board/board.json`, a `board/config/v1` record.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(rename = "$type")]
    _tag: IgnoredAny,
    #[serde(default = "commits_on_put")]
    pub commit_on_put: bool,
    #[serde(default)]
    pub plugins: Vec<PluginName>,
    /// Where bytes held outside the tree go (docs/architecture/attachments.md).
    #[serde(default)]
    pub store: Option<Store>,
}

/// The object store a board uploads large and binary files to.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Store {
    pub backend: Backend,
    /// For `directory`, the directory's path, absolute or relative to the
    /// board's root.
    pub location: String,
}

/// The backends Board offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    Directory,
}

fn commits_on_put() -> bool {
    true
}

impl Config {
    /// The configuration a parsed `board/config/v1` record holds. A plugin
    /// enabled twice is refused.
    pub(crate) fn from_value(value: &Value) -> Option<Self> {
        let config = Self::deserialize(value).ok()?;
        let mut plugins: Vec<&PluginName> = config.plugins.iter().collect();
        plugins.sort();
        plugins.dedup();
        (plugins.len() == config.plugins.len()).then_some(config)
    }
}

/// An open board: the working tree of its repository, the repositories its
/// mounts name, and its configuration.
#[derive(Debug)]
pub struct Board {
    repositories: Repositories,
    config: Config,
    pub(crate) trace: RefCell<Trace>,
}

// Board must stay `Send` so that a caller can move it to another thread, as
// a server adapter would. gix's `parallel` feature makes its repositories
// `Send`; without it this fails to compile. A Board is not `Sync`: gix keeps
// per-repository caches in `RefCell`s.
const _: () = {
    const fn send<T: Send>() {}
    send::<Board>();
};

impl Board {
    /// Opens the board whose root is `start` or the nearest directory above
    /// it holding `.board/board.json`. That directory must be the root of its
    /// repository's working tree. `home` is what a mount's `~/` means.
    pub fn open(start: &Path, home: Option<PathBuf>) -> Result<Self, OpenError> {
        let not_found = || {
            let start = start.to_path_buf();
            match board_tree::repository_root(&start) {
                Some(_) => OpenError::NotFound { start },
                None => OpenError::NoRepository { start },
            }
        };
        let root = board_tree::discover(start).ok_or_else(not_found)?;
        if !board_tree::is_repository_root(&root) {
            return Err(not_found());
        }
        let repositories =
            Repositories::open(&root, home).map_err(|source| OpenError::Git { source })?;
        if repositories.root().path() != root {
            return Err(not_found());
        }
        let config = match config_file(&repositories) {
            Ok((_, _, config)) => config,
            // A symlink on the way is named, rather than the configuration.
            Err(Some(source @ ResolveError::Symlink { .. })) => {
                return Err(OpenError::Symlink { root, source });
            }
            Err(_) => return Err(OpenError::Config { root }),
        };
        Ok(Self {
            repositories,
            config,
            trace: RefCell::default(),
        })
    }

    pub fn repository(&self) -> &Repository {
        self.repositories.root()
    }

    pub fn repositories(&self) -> &Repositories {
        &self.repositories
    }

    pub fn config(&self) -> &Config {
        &self.config
    }
}

/// The logical path of the board's configuration.
const CONFIG_PATH: &str = "/.board/board";

/// The configuration in the working tree's `.board/board.json`.
pub(crate) fn read_config(repositories: &Repositories) -> Option<Config> {
    config_file(repositories).ok().map(|(_, _, config)| config)
}

/// The working tree's `.board/board.json`: its blob, its value and the
/// configuration it holds, when it holds a valid one, else why its path
/// does not resolve, if it does not.
pub(crate) fn config_file(
    repositories: &Repositories,
) -> Result<(ObjectId, Value, Config), Option<ResolveError>> {
    let path: LogicalPath = CONFIG_PATH.parse().map_err(|_| None)?;
    let location = repositories.resolve(&Source::WorkingTree, &path)?;
    let Object::Blob(blob) = location.object else {
        return Err(None);
    };
    let root = &mut repositories.root().reader();
    let contents = latest(repositories.read(root, &location, blob));
    let contents = contents.ok().flatten().ok_or(None)?;
    match classify(&location.path, Some(&contents.bytes)) {
        Classified {
            content: Content::Record { record_type, value },
            code: None,
        } if record_type.as_str() == CONFIG_TYPE => Config::from_value(&value)
            .map(|config| (contents.blob, value, config))
            .ok_or(None),
        _ => Err(None),
    }
}

/// A board that cannot be opened.
#[derive(Debug, thiserror::Error)]
pub enum OpenError {
    #[error("no board at or above {start:?}: no .board/board.json at a repository root")]
    NotFound { start: PathBuf },
    #[error("no board at or above {start:?}: no git repository holds it")]
    NoRepository { start: PathBuf },
    #[error("the board at {root:?} has a .board/board.json that is not a valid board/config/v1")]
    Config { root: PathBuf },
    #[error("the board at {root:?} cannot read its configuration: {source}")]
    Symlink { root: PathBuf, source: ResolveError },
    #[error(transparent)]
    Git { source: GitError },
}

impl OpenError {
    pub fn code(&self) -> Code {
        match self {
            Self::NotFound { .. } | Self::NoRepository { .. } => Code::ConfigNotFound,
            Self::Config { .. } => Code::ConfigInvalid,
            Self::Symlink { source, .. } => source.code(),
            Self::Git { source } => source.code(),
        }
    }

    /// The symlink at fault, for a configuration a symlink leads nowhere.
    pub fn fields(&self) -> Fields {
        match self {
            Self::Symlink { source, .. } => source.fields(),
            _ => Fields::default(),
        }
    }
}
