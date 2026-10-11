//! Where the index lives and how it opens (docs/architecture/index.md).
//! Every way of opening ends in the same preparation, on a file or in memory.

use std::ffi::OsStr;
use std::fmt::Write as _;
use std::io::{self, Write as _};
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use board_model::Code;
use rusqlite::{Connection, ErrorCode, Transaction, TransactionBehavior};
use sha2::{Digest, Sha256};

use crate::schema::{INDEX_VERSION, TABLES};

/// How long to wait for another process's write before working in memory.
pub(crate) const BUSY_WAIT: Duration = Duration::from_secs(5);

/// The index file for the board at the canonical path `root`, under the
/// cache directory `cache`. Its name carries the format, so builds of
/// different formats keep separate files and never delete each other's.
pub fn location(cache: &Path, root: &Path) -> PathBuf {
    board(cache, root).join(format!("index-{INDEX_VERSION}.sqlite"))
}

/// The directory under `cache` that holds the cache of the board at `root`.
fn board(cache: &Path, root: &Path) -> PathBuf {
    let digest = Sha256::digest(root.as_os_str().as_encoded_bytes());
    cache.join("board").join(hex(&digest))
}

/// `bytes` in lowercase hex.
pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut hex, byte| {
        // Writing to a String cannot fail.
        let _ = write!(hex, "{byte:02x}");
        hex
    })
}

/// An open index.
#[derive(Debug)]
pub struct Index {
    pub(crate) connection: Connection,
    storage: Storage,
}

/// Where an index is kept.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Storage {
    File(PathBuf),
    /// In memory for this run only, and why.
    Memory(Fallback),
}

/// Why an index is in memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fallback {
    /// There is no cache directory.
    NoCache,
    /// Another process held the file past the wait.
    Busy,
    /// The file could not be used even after it was deleted and rebuilt.
    Unusable,
}

/// Why a file could not be prepared.
enum Failure {
    Busy,
    /// Corrupt, of another format, or unreadable: deleted and rebuilt.
    Unusable,
}

impl Index {
    /// Opens the index for the board at the canonical path `root`. A file
    /// that is corrupt or of another format is deleted and rebuilt; one held
    /// past the wait, or one that cannot be used, gives an index in memory.
    pub fn open(cache: Option<&Path>, root: &Path) -> Result<Self, IndexError> {
        let Some(cache) = cache else {
            return Self::memory(Fallback::NoCache);
        };
        match Self::file(cache, root, false) {
            Ok(index) => Ok(index),
            Err(Failure::Busy) => Self::memory(Fallback::Busy),
            Err(Failure::Unusable) => match Self::file(cache, root, true) {
                Ok(index) => Ok(index),
                Err(Failure::Busy) => Self::memory(Fallback::Busy),
                Err(Failure::Unusable) => Self::memory(Fallback::Unusable),
            },
        }
    }

    pub fn storage(&self) -> &Storage {
        &self.storage
    }

    /// An empty index in memory, as opening gives when another process
    /// holds the file past the wait.
    pub fn fall_back() -> Result<Self, IndexError> {
        Self::memory(Fallback::Busy)
    }

    fn file(cache: &Path, root: &Path, reset: bool) -> Result<Self, Failure> {
        let file = location(cache, root);
        directory(&file, reset).map_err(|_| Failure::Unusable)?;
        let mut connection = Connection::open(&file).map_err(|error| failure(&error))?;
        let created = prepare(&mut connection).map_err(|error| match error {
            Prepared::Sql(error) => failure(&error),
            Prepared::Foreign => Failure::Unusable,
        })?;
        // `transact` in board-ops relies on WAL: a run that holds the write
        // lock can then commit without being busy.
        let mode: String = connection
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .map_err(|error| failure(&error))?;
        if mode != "wal" {
            return Err(Failure::Unusable);
        }
        if created {
            // The cache is disposable, so tidying it may fail.
            let _ = tidy(cache, root);
        }
        Ok(Self {
            connection,
            storage: Storage::File(file),
        })
    }

    fn memory(fallback: Fallback) -> Result<Self, IndexError> {
        let mut connection = Connection::open_in_memory().map_err(IndexError::from)?;
        prepare(&mut connection).map_err(|error| match error {
            Prepared::Sql(error) => IndexError::from(error),
            Prepared::Foreign => IndexError::Foreign,
        })?;
        Ok(Self {
            connection,
            storage: Storage::Memory(fallback),
        })
    }
}

/// Creates the index file's directory and, when `reset`, deletes the file
/// and `SQLite`'s journals beside it.
#[expect(
    clippy::disallowed_methods,
    reason = "board-index owns the cache's files"
)]
fn directory(file: &Path, reset: bool) -> io::Result<()> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if reset {
        for suffix in ["", "-wal", "-shm"] {
            let mut path = file.as_os_str().to_owned();
            path.push(suffix);
            match std::fs::remove_file(path) {
                Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
                _ => {}
            }
        }
    }
    Ok(())
}

/// Records `root` in the cache of the board at `root`, under `cache`,
/// deletes the index files there more than one format older, and deletes the
/// cache of each other board whose recorded root is known to be gone. A root
/// that cannot be looked at may still exist, so it is kept.
#[expect(
    clippy::disallowed_methods,
    reason = "board-index owns the cache's files"
)]
fn tidy(cache: &Path, root: &Path) -> io::Result<()> {
    let directory = board(cache, root);
    // Renamed into place, so another build never reads it half written.
    let mut recording = tempfile::NamedTempFile::new_in(&directory)?;
    recording.write_all(root.as_os_str().as_bytes())?;
    recording
        .persist(directory.join("root"))
        .map_err(|error| error.error)?;
    let mut older = Vec::new();
    for entry in std::fs::read_dir(&directory)? {
        let path = entry?.path();
        let format = path.file_name().and_then(OsStr::to_str).and_then(format);
        if format.is_some_and(|format| format + 1 < INDEX_VERSION) {
            older.push(path);
        }
    }
    for path in databases_first(older) {
        std::fs::remove_file(path)?;
    }
    for board in std::fs::read_dir(cache.join("board"))? {
        let board = board?.path();
        if board == directory {
            continue;
        }
        let Ok(recorded) = std::fs::read(board.join("root")) else {
            continue;
        };
        if matches!(
            Path::new(OsStr::from_bytes(&recorded)).try_exists(),
            Ok(false)
        ) {
            std::fs::remove_dir_all(&board)?;
        }
    }
    Ok(())
}

/// The format of the index file or journal named `name`. The file from
/// before formats were in its name counts as format 0.
fn format(name: &str) -> Option<u32> {
    let format = stem(name)?.strip_prefix("index")?;
    if format.is_empty() {
        return Some(0);
    }
    format.strip_prefix('-')?.parse().ok()
}

/// The name of an `SQLite` database or journal file without its suffix.
pub(crate) fn stem(name: &str) -> Option<&str> {
    [".sqlite", ".sqlite-wal", ".sqlite-shm"]
        .into_iter()
        .find_map(|suffix| name.strip_suffix(suffix))
}

/// `paths` with each `SQLite` database before every journal, so deleting
/// them in order never leaves a database without its journal.
pub(crate) fn databases_first(mut paths: Vec<PathBuf>) -> Vec<PathBuf> {
    paths.sort_by_key(|path| path.extension() != Some(OsStr::new("sqlite")));
    paths
}

enum Prepared {
    Sql(rusqlite::Error),
    Foreign,
}

impl From<rusqlite::Error> for Prepared {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sql(error)
    }
}

/// Confirms the format of a filled database, or takes the write lock and
/// creates the tables in an empty one, and says whether it created them.
/// Confirming only reads, so it does not wait for another process's write.
fn prepare(connection: &mut Connection) -> Result<bool, Prepared> {
    connection.busy_timeout(BUSY_WAIT)?;
    // Room for every statement the writer runs, so each is compiled once.
    connection.set_prepared_statement_cache_capacity(32);
    // So a prune can give pages back to the file, and a body of a few KiB
    // fits a page. Both take effect only before the file has pages, and
    // setting them on a filled one waits to write.
    let pages: i64 = connection.query_row("PRAGMA page_count", [], |row| row.get(0))?;
    if pages == 0 {
        connection.pragma_update(None, "page_size", 16_384)?;
        connection.pragma_update(None, "auto_vacuum", "INCREMENTAL")?;
    }
    connection.pragma_update(None, "journal_mode", "WAL")?;
    if confirm(&connection.transaction()?)? {
        return Ok(false);
    }
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let created = !confirm(&transaction)?;
    if created {
        transaction.execute_batch(TABLES)?;
        transaction.execute(
            "INSERT INTO state (key, value) VALUES ('index_version', ?1)",
            [INDEX_VERSION],
        )?;
    }
    transaction.commit()?;
    Ok(created)
}

/// Whether the database holds tables, which must be of this format.
fn confirm(transaction: &Transaction<'_>) -> Result<bool, Prepared> {
    let tables: i64 =
        transaction.query_row("SELECT count(*) FROM sqlite_schema", [], |row| row.get(0))?;
    if tables == 0 {
        return Ok(false);
    }
    let version: Option<String> = transaction
        .query_row(
            "SELECT value FROM state WHERE key = 'index_version'",
            [],
            |row| row.get(0),
        )
        .ok();
    if version != Some(INDEX_VERSION.to_string()) {
        return Err(Prepared::Foreign);
    }
    Ok(true)
}

fn failure(error: &rusqlite::Error) -> Failure {
    match error.sqlite_error_code() {
        Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked) => Failure::Busy,
        _ => Failure::Unusable,
    }
}

/// An index that cannot be used even in memory, or a statement that failed.
/// Since the index is disposable, a failure here is never the tree's fault.
#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("the index failed: {source}")]
    Sql { source: rusqlite::Error },
    /// Another process held the write lock, or wrote first.
    #[error("the index is busy: {source}")]
    Busy { source: rusqlite::Error },
    #[error("a new index in memory reports another format")]
    Foreign,
    #[error("the query cannot run: {source}")]
    Query {
        #[source]
        source: rusqlite::Error,
    },
    #[error("the query ran longer than {:?}", crate::query::QUERY_TIME)]
    Timeout,
    /// One value was longer than the query's whole budget.
    #[error("the query made a value longer than one run prints")]
    TooBig,
    /// Another process pruned facts this run's views show, so the run must
    /// build them again.
    #[error("another process pruned the index after this run built its views")]
    Pruned,
}

impl IndexError {
    pub fn code(&self) -> Code {
        match self {
            Self::Sql { .. } | Self::Busy { .. } | Self::Foreign | Self::Pruned => {
                Code::IndexFailed
            }
            Self::Query { .. } => Code::QueryInvalid,
            Self::Timeout => Code::QueryTimeout,
            Self::TooBig => Code::QueryOverBudget,
        }
    }
}

impl From<rusqlite::Error> for IndexError {
    fn from(source: rusqlite::Error) -> Self {
        match failure(&source) {
            Failure::Busy => Self::Busy { source },
            Failure::Unusable => Self::Sql { source },
        }
    }
}
