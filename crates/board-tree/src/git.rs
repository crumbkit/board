//! The one module that reads git objects and references.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

use board_model::{Code, MAX_RECORD_BYTES, ObjectId, Source};
use gix::object::tree::EntryKind;

use crate::tree::{Kind, RawEntry, Tree};

/// A git repository opened for reading.
#[derive(Debug)]
pub struct Repository {
    path: PathBuf,
    git: gix::Repository,
    /// How many directories its working-tree views read from disk.
    listed: AtomicUsize,
    /// How many working-tree files it opened to hash or read.
    opened: AtomicUsize,
}

/// A commit's tree, read from the object database.
#[derive(Debug)]
pub(crate) struct CommitTree<'r> {
    repository: &'r Repository,
    commit: ObjectId,
    root: ObjectId,
}

impl Repository {
    pub(crate) fn open(path: &Path) -> Result<Self, GitError> {
        let unreadable = |source: Box<dyn Error + Send + Sync>| GitError::Unreadable {
            repository: path.to_path_buf(),
            source,
        };
        let git = gix::open(path).map_err(|error| unreadable(Box::new(error)))?;
        let path = git
            .workdir()
            .unwrap_or_else(|| git.git_dir())
            .canonicalize()
            .map_err(|error| unreadable(Box::new(error)))?;
        Ok(Self::lent(path, git))
    }

    /// The repository at the canonical `path` that `git` has open.
    pub(crate) fn lent(path: PathBuf, git: gix::Repository) -> Self {
        Self {
            path,
            git,
            listed: AtomicUsize::new(0),
            opened: AtomicUsize::new(0),
        }
    }

    /// The repository's canonical working directory, or its git directory
    /// when it has none.
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn git(&self) -> &gix::Repository {
        &self.git
    }

    /// How many directories this repository's working-tree views have read
    /// from disk. Work-count tests assert it.
    pub fn directories_listed(&self) -> usize {
        self.listed.load(Ordering::Relaxed)
    }

    /// Counts one directory read from disk.
    pub(crate) fn listed_one(&self) {
        self.listed.fetch_add(1, Ordering::Relaxed);
    }

    /// How many working-tree files this repository opened to hash or read.
    /// Work-count tests assert it.
    pub fn files_opened(&self) -> usize {
        self.opened.load(Ordering::Relaxed)
    }

    /// Counts one working-tree file opened.
    pub(crate) fn opened_one(&self) {
        self.opened.fetch_add(1, Ordering::Relaxed);
    }

    /// The commit `HEAD` names, or `None` before the first commit.
    pub(crate) fn head(&self) -> Result<Option<ObjectId>, GitError> {
        let unreadable = |source: Box<dyn Error + Send + Sync>| GitError::Unreadable {
            repository: self.path.clone(),
            source,
        };
        let mut head = self.git.head().map_err(|error| unreadable(error.into()))?;
        if head.is_unborn() {
            return Ok(None);
        }
        let id = head
            .try_peel_to_id()
            .map_err(|error| unreadable(error.into()))?;
        Ok(id.map(|id| model_id(&id)))
    }

    /// The tree of `commit`.
    pub(crate) fn commit_tree(&self, commit: ObjectId) -> Result<CommitTree<'_>, GitError> {
        let found = self
            .git
            .find_commit(self.gix_id(commit)?)
            .map_err(|error| self.missing(commit, error))?;
        let tree = found
            .tree_id()
            .map_err(|error| self.missing(commit, error))?;
        Ok(CommitTree {
            repository: self,
            commit,
            root: model_id(&tree),
        })
    }

    /// The entries of `tree`, in git's order.
    fn list_tree(&self, tree: ObjectId) -> Result<Vec<RawEntry<ObjectId>>, GitError> {
        Ok(self
            .tree_entries(tree)?
            .into_iter()
            .map(|(entry, _, _)| entry)
            .collect())
    }

    /// The entries of `tree`, in git's order, each with git's mode and the
    /// object it names.
    pub(crate) fn tree_entries(
        &self,
        tree: ObjectId,
    ) -> Result<Vec<(RawEntry<ObjectId>, u32, ObjectId)>, GitError> {
        let found = self
            .git
            .find_tree(self.gix_id(tree)?)
            .map_err(|error| self.missing(tree, error))?;
        let decoded = found.decode().map_err(|error| self.missing(tree, error))?;
        Ok(decoded
            .entries
            .iter()
            .map(|entry| {
                let id = model_id(entry.oid);
                let size = matches!(
                    entry.mode.kind(),
                    EntryKind::Blob | EntryKind::BlobExecutable | EntryKind::Link
                )
                .then(|| {
                    self.git
                        .find_header(entry.oid)
                        .ok()
                        .map(|header| header.size())
                })
                .flatten();
                let raw = RawEntry {
                    name: entry.filename.to_vec(),
                    kind: match entry.mode.kind() {
                        EntryKind::Tree => Kind::Directory(id),
                        EntryKind::Blob | EntryKind::BlobExecutable => Kind::File(id),
                        EntryKind::Link => Kind::Symlink(id),
                        EntryKind::Commit => Kind::Submodule,
                    },
                    stat: None,
                    size,
                };
                (raw, u32::from(entry.mode.value()), id)
            })
            .collect())
    }

    /// The contents of the file at the `/`-separated `path` in `source`,
    /// whose blob was `blob` when it was walked, or `None` when it holds more
    /// than [`MAX_RECORD_BYTES`]. A working-tree file that no longer holds
    /// `blob` is [`GitError::Changed`], which carries what was read.
    pub fn read(
        &self,
        source: &Source,
        path: &str,
        blob: ObjectId,
    ) -> Result<Option<Contents>, GitError> {
        match source {
            Source::Commit(_) => Ok(self.read_blob(blob)?.map(|bytes| Contents { blob, bytes })),
            Source::WorkingTree => self.read_walked(Path::new(path), Some(blob)),
        }
    }

    /// The bytes of blob `id`, or `None` when it holds more than
    /// [`MAX_RECORD_BYTES`]; the size is checked before the blob is loaded.
    pub(crate) fn read_blob(&self, id: ObjectId) -> Result<Option<Vec<u8>>, GitError> {
        let gix_id = self.gix_id(id)?;
        let header = self
            .git
            .find_header(gix_id)
            .map_err(|error| self.missing(id, error))?;
        if header.size() > MAX_RECORD_BYTES as u64 {
            return Ok(None);
        }
        let blob = self
            .git
            .find_blob(gix_id)
            .map_err(|error| self.missing(id, error))?;
        Ok(Some(blob.data.clone()))
    }

    fn gix_id(&self, id: ObjectId) -> Result<gix::ObjectId, GitError> {
        gix::ObjectId::try_from(id.as_bytes()).map_err(|error| self.missing(id, error))
    }

    fn missing(&self, id: ObjectId, source: impl Into<Box<dyn Error + Send + Sync>>) -> GitError {
        GitError::Object {
            repository: self.path.clone(),
            id,
            source: source.into(),
        }
    }
}

impl Tree for CommitTree<'_> {
    type Directory = ObjectId;

    fn repository(&self) -> &Path {
        self.repository.path()
    }

    fn source(&self) -> Source {
        Source::Commit(self.commit)
    }

    fn root(&self) -> ObjectId {
        self.root
    }

    fn list(&mut self, directory: &ObjectId) -> Result<Rc<[RawEntry<ObjectId>]>, GitError> {
        Ok(self.repository.list_tree(*directory)?.into())
    }

    fn read(
        &mut self,
        _directory: &ObjectId,
        entry: &RawEntry<ObjectId>,
    ) -> Result<Option<Vec<u8>>, GitError> {
        let Kind::File(id) = entry.kind else {
            return Ok(None);
        };
        self.repository.read_blob(id)
    }
}

pub(crate) fn model_id(id: &gix::oid) -> ObjectId {
    let bytes = id.as_bytes();
    match <[u8; 32]>::try_from(bytes) {
        Ok(sha256) => ObjectId::Sha256(sha256),
        Err(_) => ObjectId::Sha1(
            <[u8; 20]>::try_from(bytes)
                .expect("a git object id that is not SHA-256 is SHA-1's 20 bytes"),
        ),
    }
}

/// A file's bytes and the blob they make.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Contents {
    pub blob: ObjectId,
    pub bytes: Vec<u8>,
}

/// The contents `read` found, taking those of a file that changed since it
/// was walked as they are now.
pub fn latest(read: Result<Option<Contents>, GitError>) -> Result<Option<Contents>, GitError> {
    match read {
        Err(GitError::Changed { contents, .. }) => Ok(Some(contents)),
        read => read,
    }
}

/// A repository or object that could not be read.
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("{repository:?} cannot be opened as a git repository")]
    Unreadable {
        repository: PathBuf,
        #[source]
        source: Box<dyn Error + Send + Sync>,
    },
    #[error("{repository:?} has no working tree")]
    NoWorkingTree { repository: PathBuf },
    #[error("{path:?} cannot be read")]
    File {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{path:?} changed while it was read; run the command again")]
    Changed { path: PathBuf, contents: Contents },
    #[error("object {id} in {repository:?} cannot be read")]
    Object {
        repository: PathBuf,
        id: ObjectId,
        #[source]
        source: Box<dyn Error + Send + Sync>,
    },
}

impl GitError {
    pub fn code(&self) -> Code {
        match self {
            Self::Unreadable { .. }
            | Self::NoWorkingTree { .. }
            | Self::File { .. }
            | Self::Changed { .. }
            | Self::Object { .. } => Code::RepoUnreadable,
        }
    }
}
