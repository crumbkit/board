//! Where the index lives and how it opens (docs/architecture.md, Index).
//! Every way of opening ends in the same preparation, on a file or in memory.

use std::fmt::Write as _;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, ErrorCode, TransactionBehavior};
use sha2::{Digest, Sha256};

use crate::schema::{INDEX_VERSION, TABLES};

/// How long to wait for another process's write before working in memory.
const BUSY_WAIT: Duration = Duration::from_secs(5);

/// The index file for the board at the canonical path `root`, under the
/// cache directory `cache`.
pub fn location(cache: &Path, root: &Path) -> PathBuf {
    let digest = Sha256::digest(root.as_os_str().as_encoded_bytes());
    let hex = digest.iter().fold(String::new(), |mut hex, byte| {
        // Writing to a String cannot fail.
        let _ = write!(hex, "{byte:02x}");
        hex
    });
    cache.join("board").join(hex).join("index.sqlite")
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
        let file = location(cache, root);
        match Self::file(&file, false) {
            Ok(index) => Ok(index),
            Err(Failure::Busy) => Self::memory(Fallback::Busy),
            Err(Failure::Unusable) => match Self::file(&file, true) {
                Ok(index) => Ok(index),
                Err(Failure::Busy) => Self::memory(Fallback::Busy),
                Err(Failure::Unusable) => Self::memory(Fallback::Unusable),
            },
        }
    }

    pub fn storage(&self) -> &Storage {
        &self.storage
    }

    fn file(file: &Path, reset: bool) -> Result<Self, Failure> {
        directory(file, reset).map_err(|_| Failure::Unusable)?;
        let mut connection = Connection::open(file).map_err(|error| failure(&error))?;
        prepare(&mut connection).map_err(|error| match error {
            Prepared::Sql(error) => failure(&error),
            Prepared::Foreign => Failure::Unusable,
        })?;
        Ok(Self {
            connection,
            storage: Storage::File(file.to_path_buf()),
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

enum Prepared {
    Sql(rusqlite::Error),
    Foreign,
}

impl From<rusqlite::Error> for Prepared {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sql(error)
    }
}

/// Takes the write lock, then creates the tables in an empty database or
/// confirms the format of a filled one.
fn prepare(connection: &mut Connection) -> Result<(), Prepared> {
    connection.busy_timeout(BUSY_WAIT)?;
    connection.pragma_update(None, "journal_mode", "WAL")?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let tables: i64 =
        transaction.query_row("SELECT count(*) FROM sqlite_schema", [], |row| row.get(0))?;
    if tables == 0 {
        transaction.execute_batch(TABLES)?;
        transaction.execute(
            "INSERT INTO state (key, value) VALUES ('index_version', ?1)",
            [INDEX_VERSION],
        )?;
    } else {
        let version: Option<String> = transaction
            .query_row(
                "SELECT value FROM state WHERE key = 'index_version'",
                [],
                |row| row.get(0),
            )
            .ok();
        if version.as_deref() != Some(INDEX_VERSION) {
            return Err(Prepared::Foreign);
        }
    }
    transaction.commit()?;
    Ok(())
}

fn failure(error: &rusqlite::Error) -> Failure {
    match error.sqlite_error_code() {
        Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked) => Failure::Busy,
        _ => Failure::Unusable,
    }
}

/// An index that cannot be used even in memory, or a statement that failed.
#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("the index failed: {source}")]
    Sql {
        #[from]
        source: rusqlite::Error,
    },
    #[error("a new index in memory reports another format")]
    Foreign,
}
