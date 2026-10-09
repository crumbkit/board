//! Each plugin version's database file (docs/plugins.md, Databases). The
//! plugin owns what is inside but one table, where Board keeps the id that
//! tells this database from one made again at the same path.

use std::ffi::OsStr;
use std::fs::OpenOptions;
use std::io;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension as _};
use tempfile::TempDir;

use crate::open::{BUSY_WAIT, Storage, databases_first, stem};

/// A plugin database's path, and the temporary directory that holds it when
/// it is not kept. The directory is removed when this is dropped.
#[derive(Debug)]
pub struct PluginDatabase {
    file: PathBuf,
    id: Option<String>,
    fallback: bool,
    _temporary: Option<TempDir>,
}

impl PluginDatabase {
    /// The database of `plugin` at `version`, in a `plugins` directory beside
    /// an index kept in a file, or in a temporary directory beside one in
    /// memory or when that directory cannot be made. A missing file is
    /// created empty, which `SQLite` reads as an empty database. Opening it
    /// in the kept directory deletes the databases of the plugin's older
    /// versions. A database with no id that `SQLite` cannot write, or one it
    /// cannot read, has no id.
    #[expect(
        clippy::disallowed_methods,
        reason = "board-index owns the cache's files"
    )]
    pub fn open(storage: &Storage, plugin: &str, version: &str) -> Result<Self, DatabaseError> {
        let kept = match storage {
            Storage::File(index) => index
                .parent()
                .map(|directory| directory.join("plugins"))
                .filter(|directory| std::fs::create_dir_all(directory).is_ok()),
            Storage::Memory(_) => None,
        };
        let (directory, temporary) = if let Some(directory) = kept {
            for older in superseded(&directory, plugin, version) {
                // The databases are disposable, so deleting one may fail.
                let _ = std::fs::remove_file(older);
            }
            (directory, None)
        } else {
            let temporary =
                tempfile::tempdir().map_err(|source| DatabaseError::TemporaryDirectory {
                    parent: std::env::temp_dir(),
                    source,
                })?;
            (temporary.path().to_path_buf(), Some(temporary))
        };
        let file = directory.join(format!("{plugin}@{version}.sqlite"));
        match OpenOptions::new().append(true).create_new(true).open(&file) {
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(source) => return Err(DatabaseError::File { path: file, source }),
            Ok(created) => drop(created),
        }
        Ok(Self {
            id: identify(&file).ok(),
            file,
            fallback: temporary.is_some() && matches!(storage, Storage::File(_)),
            _temporary: temporary,
        })
    }

    pub fn path(&self) -> &Path {
        &self.file
    }

    /// The id Board keeps in this database, which a database deleted and
    /// made again at the same path does not share.
    pub fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }

    /// Whether the index is kept but this database could not be, so it is a
    /// temporary file for this run.
    pub fn fallback(&self) -> bool {
        self.fallback
    }
}

/// A plugin database Board could not make, and why.
#[derive(Debug, thiserror::Error)]
pub enum DatabaseError {
    /// The database file at `path` could not be created.
    #[error("{} cannot be made: {source}", path.display())]
    File { path: PathBuf, source: io::Error },
    /// No temporary directory could be made in `parent` to hold it.
    #[error("a temporary directory cannot be made in {}: {source}", parent.display())]
    TemporaryDirectory { parent: PathBuf, source: io::Error },
}

/// The id Board keeps in the database at `file`, made at random when the
/// database holds none. Reading an id does not wait for the plugin's write.
fn identify(file: &Path) -> rusqlite::Result<String> {
    let connection = Connection::open(file)?;
    connection.busy_timeout(BUSY_WAIT)?;
    if let Some(id) = read_id(&connection)? {
        return Ok(id);
    }
    connection.execute_batch(
        "BEGIN IMMEDIATE;
         CREATE TABLE IF NOT EXISTS board_database (id TEXT NOT NULL);
         INSERT INTO board_database SELECT lower(hex(randomblob(16)))
             WHERE NOT EXISTS (SELECT 1 FROM board_database);
         COMMIT;",
    )?;
    connection.query_row("SELECT id FROM board_database", [], |row| row.get(0))
}

/// The id in `connection`'s database, if it holds one.
fn read_id(connection: &Connection) -> rusqlite::Result<Option<String>> {
    let exists: bool = connection.query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_schema WHERE name = 'board_database')",
        [],
        |row| row.get(0),
    )?;
    if !exists {
        return Ok(None);
    }
    connection
        .query_row("SELECT id FROM board_database", [], |row| row.get(0))
        .optional()
}

/// The files in `directory` that hold `plugin`'s databases of versions older
/// than `version`, with their journals, each database before the journals.
fn superseded(directory: &Path, plugin: &str, version: &str) -> Vec<PathBuf> {
    let (Some(opened), Ok(entries)) = (numbers(version), std::fs::read_dir(directory)) else {
        return Vec::new();
    };
    let prefix = format!("{plugin}@");
    let paths = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(OsStr::to_str)
                .and_then(stem)
                .and_then(|stem| stem.strip_prefix(&prefix))
                .and_then(numbers)
                .is_some_and(|older| older < opened)
        })
        .collect();
    databases_first(paths)
}

/// The three numbers of a version `MAJOR.MINOR.PATCH`, in the order that
/// compares them.
fn numbers(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.split('.').map(|part| part.parse().ok());
    let numbers = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(numbers)
}
