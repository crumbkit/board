//! Finding and opening a board (docs/architecture.md, Boards and entries).

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use board_model::{
    CONFIG_TYPE, Classified, Code, Content, Fields, LogicalPath, Object, ObjectId, PluginName,
    Source, classify,
};
use board_tree::{Checkout, GitError, Repositories, Repository, ResolveError, latest};
use serde::Deserialize;
use serde::de::IgnoredAny;
use serde_json::Value;

use crate::effects::Asker;
use crate::host::BuiltIn;
use crate::trace::Trace;
use crate::version::{Built, MinBoard, Running, Version};

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
    /// The oldest Board that may write this board, which every older Board
    /// refuses to write or sync (docs/architecture/writes.md).
    #[serde(default)]
    pub min_board: Option<MinBoard>,
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
    pub(crate) asker: Asker,
    pub(crate) built_in: BuiltIn,
    /// What built the code that decides, which keys the decisions the index
    /// keeps.
    pub(crate) build: &'static str,
    /// The Board this run is, which `min_board` is compared with.
    pub(crate) running: Running,
    /// The mount whose checkout this run started in, when Board found the
    /// board through it: see [`Board::started_in`].
    started: Option<(LogicalPath, PathBuf)>,
    /// Why this run's commits touch many files, which each commit gives in
    /// a `Board-Bulk:` trailer: see [`Board::mark_bulk`].
    pub(crate) bulk: Option<crate::Bulk>,
    /// Runs before each attempt to move HEAD for a put's commit. Nothing in
    /// production; a test uses it to move HEAD at that moment.
    pub(crate) between: Between,
}

/// What a put runs, given the attempt number, after staging its commit and
/// before moving HEAD to it.
pub(crate) struct Between(Box<dyn Fn(usize) + Send>);

impl Between {
    pub(crate) fn new(run: impl Fn(usize) + Send + 'static) -> Self {
        Self(Box::new(run))
    }

    pub(crate) fn run(&self) -> &dyn Fn(usize) {
        &*self.0
    }
}

impl Default for Between {
    fn default() -> Self {
        Self::new(|_| {})
    }
}

impl std::fmt::Debug for Between {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Between")
    }
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
            Err(_) => {
                let detail = config_problem(&repositories);
                return Err(OpenError::Config { root, detail });
            }
        };
        let mut board = Self {
            repositories,
            config,
            trace: RefCell::default(),
            asker: Asker::default(),
            built_in: BuiltIn::default(),
            build: "",
            running: Running {
                version: Version::from_str(env!("CARGO_PKG_VERSION")).unwrap_or_default(),
                built: None,
            },
            started: None,
            bulk: None,
            between: Between::default(),
        };
        board.gate_writes();
        Ok(board)
    }

    /// Names the Board this run is: its version, as `major.minor.patch`
    /// with an optional `-` or `+` suffix, and the commit date of its
    /// source in seconds since the Unix epoch, when the build embedded one.
    /// A board whose `min_board` this Board is older than refuses every
    /// write and sync from now on. Text that is no version leaves the
    /// running version as it was.
    pub fn running(&mut self, version: &str, built: Option<i64>) {
        if let Ok(version) = version.parse() {
            self.running_as(Running {
                version,
                built: built.map(Built::from_seconds),
            });
        }
    }

    /// [`Board::running`] for a Board already parsed.
    pub(crate) fn running_as(&mut self, running: Running) {
        self.running = running;
        self.gate_writes();
    }

    /// Refuses writes in the repositories when `min_board` is newer than
    /// the running version, and lifts the refusal when it is not.
    fn gate_writes(&mut self) {
        let refusal = self
            .config
            .min_board
            .filter(|minimum| minimum.refuses(&self.running))
            .map(|minimum| (minimum.to_string(), self.running.to_string()));
        self.repositories.refuse_writes(refusal);
    }

    /// Marks every commit this board makes from now on as a bulk one,
    /// whose message ends in the trailer `Board-Bulk: <reason>`.
    pub fn mark_bulk(&mut self, bulk: crate::Bulk) {
        self.bulk = Some(bulk);
    }

    pub fn repository(&self) -> &Repository {
        self.repositories.root()
    }

    pub fn repositories(&self) -> &Repositories {
        &self.repositories
    }

    /// Reads the mount pointer `checkout` names at its commit for the rest
    /// of this run, instead of the commit the pointer pins, and refuses
    /// every write.
    pub(crate) fn read_checkout(&mut self, checkout: Checkout) {
        self.repositories.read_at(checkout);
    }

    /// Follows `moved` when a resolution names nothing.
    pub(crate) fn moving(&mut self, moved: Vec<board_model::Moved>) {
        self.repositories.moving(moved);
    }

    /// Records that this run started in `directory`, in a checkout of the
    /// repository the mount at `mount` reads.
    pub(crate) fn started_from(&mut self, mount: LogicalPath, directory: PathBuf) {
        self.started = Some((mount, directory));
    }

    /// The checkout of a mounted repository this run started in, when Board
    /// found the board through that mount and reads it at its pin. A run
    /// under `--checkout`, or one started in the board's own tree, has
    /// none. HEAD is read on each call, and the working tree only when
    /// [`StartedIn::uncommitted`] is asked.
    pub fn started_in(&self) -> Option<StartedIn> {
        if self.repositories.checkout().is_some() {
            return None;
        }
        let (mount, directory) = self.started.as_ref()?;
        Some(StartedIn {
            mount: mount.clone(),
            head: board_tree::head_of(directory)?,
            directory: directory.clone(),
        })
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Names the build deciding for this board, such as board-cli's
    /// version, so the index keeps no decision across builds.
    pub fn built_by(&mut self, build: &'static str) {
        self.build = build;
    }
}

/// A checkout of a mounted repository that a run started in, while the
/// run reads the mount at the commit its pointer pins.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartedIn {
    /// The mount's display path, such as `/repos/lib`.
    pub mount: LogicalPath,
    /// The commit HEAD names in the checkout.
    pub head: ObjectId,
    /// The directory the run started in.
    directory: PathBuf,
}

impl StartedIn {
    /// Whether a file git tracks in the checkout differs from HEAD, as
    /// `board_tree::uncommitted` reads it, or `None` when git's files
    /// cannot be read. It reads every tracked file's metadata, so a caller
    /// asks only when the answer can change what it reports.
    pub fn uncommitted(&self) -> Option<bool> {
        board_tree::uncommitted(&self.directory)
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

/// What is wrong with the configuration that `config_file` refused, as
/// serde reports it, such as the `min_board` that is not a version, or
/// empty when the file is not a record that parses as the configuration.
fn config_problem(repositories: &Repositories) -> String {
    let problem = || -> Option<String> {
        let path: LogicalPath = CONFIG_PATH.parse().ok()?;
        let location = repositories.resolve(&Source::WorkingTree, &path).ok()?;
        let Object::Blob(blob) = location.object else {
            return None;
        };
        let root = &mut repositories.root().reader();
        let contents = latest(repositories.read(root, &location, blob)).ok()??;
        let value: Value = serde_json::from_slice(&contents.bytes).ok()?;
        Config::deserialize(&value)
            .err()
            .map(|error| error.to_string())
    };
    problem().map_or_else(String::new, |text| format!(": {text}"))
}

/// A board that cannot be opened.
#[derive(Debug, thiserror::Error)]
pub enum OpenError {
    #[error("no board at or above {start:?}: no .board/board.json at a repository root")]
    NotFound { start: PathBuf },
    #[error("no board at or above {start:?}: no git repository holds it")]
    NoRepository { start: PathBuf },
    #[error(
        "the board at {root:?} has a .board/board.json that is not a valid board/config/v1{detail}"
    )]
    Config { root: PathBuf, detail: String },
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
