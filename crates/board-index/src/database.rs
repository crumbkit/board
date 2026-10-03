//! Each plugin version's database file (docs/plugins.md, Databases). The
//! plugin owns what is inside; Board only says where it lives.

use std::fs::OpenOptions;
use std::io;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use crate::open::Storage;

/// A plugin database's path, and the temporary directory that holds it when
/// it is not kept. The directory is removed when this is dropped.
#[derive(Debug)]
pub struct PluginDatabase {
    file: PathBuf,
    existed: bool,
    fallback: bool,
    _temporary: Option<TempDir>,
}

impl PluginDatabase {
    /// The database named `file_name` in a `plugins` directory beside an
    /// index kept in a file, or in a temporary directory beside one in
    /// memory or when that directory cannot be made. A missing file is
    /// created empty, which `SQLite` reads as an empty database.
    #[expect(
        clippy::disallowed_methods,
        reason = "board-index owns the cache's files"
    )]
    pub fn open(storage: &Storage, file_name: &str) -> io::Result<Self> {
        let kept = match storage {
            Storage::File(index) => index
                .parent()
                .map(|directory| directory.join("plugins"))
                .filter(|directory| std::fs::create_dir_all(directory).is_ok()),
            Storage::Memory(_) => None,
        };
        let (directory, temporary) = if let Some(directory) = kept {
            (directory, None)
        } else {
            let temporary = tempfile::tempdir()?;
            (temporary.path().to_path_buf(), Some(temporary))
        };
        let file = directory.join(file_name);
        let existed = file.exists();
        OpenOptions::new().append(true).create(true).open(&file)?;
        Ok(Self {
            file,
            existed,
            fallback: temporary.is_some() && matches!(storage, Storage::File(_)),
            _temporary: temporary,
        })
    }

    pub fn path(&self) -> &Path {
        &self.file
    }

    /// Whether the file was there before it was opened.
    pub fn existed(&self) -> bool {
        self.existed
    }

    /// Whether the index is kept but this database could not be, so it is a
    /// temporary file for this run.
    pub fn fallback(&self) -> bool {
        self.fallback
    }
}
