//! The one module that reads the repository's files.

use std::io;
use std::path::{Path, PathBuf};

/// The canonical path of the repository being linted.
#[derive(Debug)]
pub(crate) struct Root(PathBuf);

impl Root {
    pub(crate) fn open(path: &Path) -> Result<Self, FilesError> {
        canonical(path).map(Self)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }

    /// Reads a file named relative to the root.
    pub(crate) fn read(&self, relative: &str) -> Result<String, FilesError> {
        read(&self.0.join(relative))
    }

    /// Resolves an `@path` import written in the file `from` the way Claude
    /// Code does: relative to the importing file. An import that names no file
    /// inside the root is not part of this repository's context, so it is
    /// `None`.
    pub(crate) fn import(&self, from: &Path, import: &str) -> Option<PathBuf> {
        if import.starts_with('~') || Path::new(import).is_absolute() {
            return None;
        }
        let target = from.parent()?.join(import).canonicalize().ok()?;
        (target.starts_with(&self.0) && target.is_file()).then_some(target)
    }

    /// Every file matching a glob pattern written relative to the root.
    pub(crate) fn glob(&self, pattern: &str) -> Result<Vec<PathBuf>, FilesError> {
        let absolute = self.0.join(pattern);
        let text = absolute.to_str().ok_or_else(|| FilesError::NotUtf8 {
            path: absolute.clone(),
        })?;
        let matches = glob::glob(text).map_err(|source| FilesError::Pattern {
            pattern: pattern.to_owned(),
            source,
        })?;
        let mut files = Vec::new();
        for entry in matches {
            let path = entry.map_err(|error| FilesError::Unreadable {
                path: error.path().to_path_buf(),
                source: error.into(),
            })?;
            if path.is_file() {
                files.push(path);
            }
        }
        files.sort();
        Ok(files)
    }

    /// Every `.rs` file below the root, skipping `target` and hidden
    /// directories, in sorted order.
    pub(crate) fn rust_sources(&self) -> Result<Vec<PathBuf>, FilesError> {
        let mut pending = vec![self.0.clone()];
        let mut sources = Vec::new();
        while let Some(directory) = pending.pop() {
            let unreadable = |source| FilesError::Unreadable {
                path: directory.clone(),
                source,
            };
            for entry in std::fs::read_dir(&directory).map_err(unreadable)? {
                let entry = entry.map_err(unreadable)?;
                let kind = entry.file_type().map_err(unreadable)?;
                let path = entry.path();
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if kind.is_dir() && name != "target" && !name.starts_with('.') {
                    pending.push(path);
                } else if kind.is_file()
                    && path.extension().is_some_and(|extension| extension == "rs")
                {
                    sources.push(path);
                }
            }
        }
        sources.sort();
        Ok(sources)
    }

    /// `path` relative to the root, for reports.
    pub(crate) fn relative<'a>(&self, path: &'a Path) -> &'a Path {
        path.strip_prefix(&self.0).unwrap_or(path)
    }

    /// `path` relative to the root, as text for reports.
    pub(crate) fn display(&self, path: &Path) -> String {
        self.relative(path).display().to_string()
    }
}

pub(crate) fn read(path: &Path) -> Result<String, FilesError> {
    std::fs::read_to_string(path).map_err(|source| FilesError::Unreadable {
        path: path.to_path_buf(),
        source,
    })
}

pub(crate) fn canonical(path: &Path) -> Result<PathBuf, FilesError> {
    path.canonicalize()
        .map_err(|source| FilesError::Unreadable {
            path: path.to_path_buf(),
            source,
        })
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum FilesError {
    #[error("cannot read {}", .path.display())]
    Unreadable {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("{} is not valid UTF-8, so no glob can be written for it", .path.display())]
    NotUtf8 { path: PathBuf },
    #[error("{pattern} is not a valid glob pattern")]
    Pattern {
        pattern: String,
        #[source]
        source: glob::PatternError,
    },
}

impl FilesError {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::Unreadable { .. } => "xtask::files::unreadable",
            Self::NotUtf8 { .. } => "xtask::files::not_utf8",
            Self::Pattern { .. } => "xtask::files::bad_glob",
        }
    }
}
