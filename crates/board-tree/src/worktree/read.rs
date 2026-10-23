//! Reading one working-tree file or directory as a walk would find it.

use std::fs;
use std::io::{self, BufReader, Read};
use std::path::{Path, PathBuf};

use board_model::{MAX_RECORD_BYTES, ObjectId, Source};
use gix::bstr::ByteSlice;

use crate::git::{Contents, GitError, Repository};
use crate::symlink::{Chains, End, Refused};
use crate::worktree::{WorkingTree, ignored, names, submodule};

/// A working-tree directory as [`Repository::working_directory`] finds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Directory {
    /// A submodule, or a path beneath one.
    Submodule,
    /// The walk lists nothing at this path.
    Entries,
    /// The walk leaves out this path, or a directory above it, because it
    /// is untracked and ignored.
    Ignored,
}

/// A working-tree file as [`Repository::read_working`] finds it.
#[derive(Debug, PartialEq, Eq)]
pub enum Working {
    /// Nothing is at the path, or a path continues through a file.
    Missing,
    /// The file holds more than [`MAX_RECORD_BYTES`].
    TooLarge,
    Read(Contents),
}

impl Repository {
    /// The working-tree file at `relative`. A symlink's contents are its
    /// text, as git stores it. Nothing is beneath a symlink, since Board
    /// does not follow one into a directory, so a read never leaves the
    /// tree.
    pub fn read_working(&self, relative: impl AsRef<Path>) -> Result<Working, GitError> {
        let relative = relative.as_ref();
        let path = self.path().join(relative);
        let unreadable = |source| GitError::File {
            path: path.clone(),
            source,
        };
        let linked = |ancestor: &Path| {
            let metadata = fs::symlink_metadata(self.path().join(ancestor));
            metadata.is_ok_and(|metadata| metadata.is_symlink())
        };
        let mut ancestors = relative.ancestors().skip(1);
        if ancestors.any(|ancestor| !ancestor.as_os_str().is_empty() && linked(ancestor)) {
            return Ok(Working::Missing);
        }
        let metadata = match fs::symlink_metadata(&path) {
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
                ) =>
            {
                return Ok(Working::Missing);
            }
            metadata => metadata.map_err(unreadable)?,
        };
        let mut bytes = Vec::new();
        if metadata.is_symlink() {
            let target = fs::read_link(&path).map_err(unreadable)?;
            bytes = gix::path::into_bstr(target).to_vec();
        } else {
            self.opened_one();
            fs::File::open(&path)
                .and_then(|file| {
                    file.take(MAX_RECORD_BYTES as u64 + 1)
                        .read_to_end(&mut bytes)
                })
                .map_err(unreadable)?;
        }
        if bytes.len() > MAX_RECORD_BYTES {
            return Ok(Working::TooLarge);
        }
        Ok(Working::Read(Contents {
            blob: self.blob_id(&bytes),
            bytes,
        }))
    }

    /// [`Reader::read_through`] in `tree`, whose listings it shares, with
    /// `chains`, the places following has reached in `tree`, so the file a
    /// symlink leads to is found as `tree` finds it, a chain is followed
    /// once however many reads reach it, and the file system never follows
    /// the symlink.
    fn through<T: Send>(
        &self,
        tree: &mut WorkingTree<'_, '_, T>,
        chains: &mut Chains<PathBuf>,
        relative: &Path,
    ) -> Result<(Working, bool), GitError> {
        let link = chains.find(tree, &gix::path::into_bstr(relative))?;
        let path: PathBuf = match chains.follow(tree, link)? {
            None => return Ok((self.read_working(relative)?, false)),
            Some((End::File { at, .. }, _)) => (chains.names(at).into_iter())
                .map(|name| gix::path::from_bstr(name.as_bstr()))
                .collect(),
            Some((End::Directory(_), _)) => relative.into(),
            Some((End::Refused(code), target)) => {
                return Err(GitError::Symlink {
                    path: relative.into(),
                    refused: Refused { code, target },
                });
            }
        };
        Ok((self.read_working(path)?, true))
    }

    /// What the walk finds on the way to `relative`, by its rules: left out
    /// when it or a directory above it is untracked and ignored, a
    /// submodule when it or a directory above it is one, else a directory
    /// of entries, which includes a path beneath a symlink and anything the
    /// walk does not list, such as a FIFO, or a name that a case-insensitive
    /// file system finds under another spelling. `None` when a file or
    /// symlink is there, or nothing is, or a file is on the way. A directory
    /// on the way that cannot be listed, or whose ignore rules cannot be
    /// read, is [`GitError::File`].
    pub fn working_directory(
        &self,
        relative: impl AsRef<Path>,
    ) -> Result<Option<Directory>, GitError> {
        let index = self.index()?;
        let mut excludes = self.excludes(&index)?;
        let mut components = relative.as_ref().components().peekable();
        let mut at = PathBuf::new();
        let mut listed = names(self.path()).map_err(|source| GitError::File {
            path: self.path().to_path_buf(),
            source,
        })?;
        while let Some(component) = components.next() {
            at.push(component);
            let absolute = self.path().join(&at);
            let Ok(metadata) = fs::symlink_metadata(&absolute) else {
                return Ok(None);
            };
            if !listed.iter().any(|name| name == component.as_os_str()) {
                return Ok(Some(Directory::Entries));
            }
            let unreadable = |source| GitError::File {
                path: absolute.clone(),
                source,
            };
            if ignored(&index, &mut excludes, &at, metadata.is_dir()).map_err(unreadable)? {
                return Ok(Some(Directory::Ignored));
            }
            if !metadata.is_dir() {
                // The walk lists only files and symlinks, and does not
                // follow a symlink.
                let listed =
                    metadata.is_file() || metadata.is_symlink() && components.peek().is_none();
                return Ok((!listed).then_some(Directory::Entries));
            }
            if submodule(&index, &at, absolute.join(".git").exists()) {
                return Ok(Some(Directory::Submodule));
            }
            listed = names(&absolute).map_err(unreadable)?;
        }
        Ok(Some(Directory::Entries))
    }

    /// The blob of the working-tree file at `relative`, hashed in constant
    /// memory, for a file too large to read.
    pub fn working_blob(&self, relative: impl AsRef<Path>) -> Result<ObjectId, GitError> {
        let path = self.path().join(relative);
        let hashed = fs::File::open(&path).and_then(|file| {
            let length = file.metadata()?.len();
            self.hash(&mut BufReader::new(file), length)
                .ok_or_else(|| io::Error::other("the file changed while it was hashed"))
        });
        hashed.map_err(|source| GitError::File { path, source })
    }

    /// The contents of the working-tree file at `relative` that was walked
    /// with blob `walked`, or `None` when it is too large: read through a
    /// symlink there, as the snapshot and places `tree` gives follow it,
    /// when its own text is not `walked`. One that is gone is
    /// [`GitError::File`], and one that no longer holds `walked` is
    /// [`GitError::Changed`].
    pub(crate) fn read_walked<'t, 'r: 't, 'k: 't, T: Send + 'k>(
        &self,
        tree: impl FnOnce() -> Result<Snapshot<'t, 'r, 'k, T>, GitError>,
        relative: &Path,
        walked: Option<ObjectId>,
    ) -> Result<Option<Contents>, GitError> {
        let mut working = self.read_working(relative)?;
        if matches!(&working, Working::Read(contents) if walked != Some(contents.blob)) {
            let (tree, chains) = tree()?;
            working = self.through(tree, chains, relative)?.0;
        }
        self.as_walked(relative, working, walked)
    }

    /// `working`, read at `relative`, as the file walked with blob
    /// `walked`, without following a symlink: its contents, `None` when it
    /// is too large, [`GitError::File`] when it is gone and
    /// [`GitError::Changed`] when it no longer holds `walked`.
    pub(crate) fn as_walked(
        &self,
        relative: &Path,
        working: Working,
        walked: Option<ObjectId>,
    ) -> Result<Option<Contents>, GitError> {
        let changed = |contents: &Contents| walked.is_some_and(|blob| blob != contents.blob);
        match working {
            Working::Read(contents) if changed(&contents) => Err(GitError::Changed {
                path: self.path().join(relative),
                contents,
            }),
            Working::Read(contents) => Ok(Some(contents)),
            Working::TooLarge => Ok(None),
            Working::Missing => Err(GitError::File {
                path: self.path().join(relative),
                source: io::ErrorKind::NotFound.into(),
            }),
        }
    }
}

/// A working-tree snapshot and the places following has reached in it.
pub(crate) type Snapshot<'t, 'r, 'k, T> = (&'t mut WorkingTree<'r, 'k, T>, &'t mut Chains<PathBuf>);

/// Reads the files of one repository's sources for many walked entries.
/// A working-tree symlink is followed through one snapshot that the first
/// symlink a read meets makes and every later read shares, with the places
/// following has reached, so reading N symlinks lists each directory on
/// their way once and follows a chain they share once.
pub struct Reader<'r> {
    repository: &'r Repository,
    tree: Option<WorkingTree<'r, 'static>>,
    pub(crate) chains: Chains<PathBuf>,
}

impl std::fmt::Debug for Reader<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Reader")
            .field("repository", &self.repository.path())
            .finish_non_exhaustive()
    }
}

impl Repository {
    pub fn reader(&self) -> Reader<'_> {
        Reader {
            repository: self,
            tree: None,
            chains: Chains::default(),
        }
    }
}

impl<'r> Reader<'r> {
    /// The contents of the file at the `/`-separated `path` in `source`,
    /// whose blob was `blob` when it was walked, or `None` when it holds
    /// more than [`MAX_RECORD_BYTES`]. A working-tree file that no longer
    /// holds `blob` is [`GitError::Changed`], which carries what was read.
    pub fn read(
        &mut self,
        source: &Source,
        path: &str,
        blob: ObjectId,
    ) -> Result<Option<Contents>, GitError> {
        let repository = self.repository;
        if let Source::Commit(_) = source {
            return Ok((repository.read_blob(blob)?).map(|bytes| Contents { blob, bytes }));
        }
        repository.read_walked(|| self.snapshot(), Path::new(path), Some(blob))
    }

    /// The working-tree file at the `/`-separated `relative`, read through
    /// a symlink there as a walk follows it, and whether a symlink is
    /// there. A symlink that leads to no file is [`GitError::Symlink`]; one
    /// that leads to a directory reads as its text.
    pub fn read_through(&mut self, relative: &str) -> Result<(Working, bool), GitError> {
        let repository = self.repository;
        let linked = fs::symlink_metadata(repository.path().join(relative));
        if !linked.is_ok_and(|metadata| metadata.is_symlink()) {
            return Ok((repository.read_working(relative)?, false));
        }
        let (tree, chains) = self.snapshot()?;
        repository.through(tree, chains, Path::new(relative))
    }

    /// The snapshot, made the first time it is needed.
    fn snapshot(&mut self) -> Result<Snapshot<'_, 'r, 'static, ()>, GitError> {
        let tree = match self.tree.take() {
            Some(tree) => tree,
            None => self.repository.working_tree(None)?.on_demand(),
        };
        Ok((self.tree.insert(tree), &mut self.chains))
    }
}
