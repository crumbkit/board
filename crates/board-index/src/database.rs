//! Each plugin version's database file (docs/plugins.md, Databases). The
//! plugin owns what is inside; Board only says where it lives.

use std::io;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use crate::open::location;

/// A plugin database's path, and the temporary directory that holds it when
/// there is no cache. The directory is removed when this is dropped.
#[derive(Debug)]
pub struct PluginDatabase {
    file: PathBuf,
    _temporary: Option<TempDir>,
}

impl PluginDatabase {
    /// The database named `file_name` beside the index of the board at
    /// `root`, in a `plugins` directory Board creates, or in a temporary
    /// directory when there is no cache or that one cannot be made.
    #[expect(
        clippy::disallowed_methods,
        reason = "board-index owns the cache's files"
    )]
    pub fn open(cache: Option<&Path>, root: &Path, file_name: &str) -> io::Result<Self> {
        if let Some(directory) = cache.and_then(|cache| {
            location(cache, root)
                .parent()
                .map(|index| index.join("plugins"))
        }) && std::fs::create_dir_all(&directory).is_ok()
        {
            return Ok(Self {
                file: directory.join(file_name),
                _temporary: None,
            });
        }
        let temporary = tempfile::tempdir()?;
        Ok(Self {
            file: temporary.path().join(file_name),
            _temporary: Some(temporary),
        })
    }

    pub fn path(&self) -> &Path {
        &self.file
    }
}
