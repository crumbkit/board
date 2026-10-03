//! The one module that reads git objects and references.

use std::error::Error;
use std::path::{Path, PathBuf};

use board_model::{Code, MAX_RECORD_BYTES, ObjectId, Source};
use gix::object::tree::EntryKind;

use crate::tree::{Kind, RawEntry, Tree};

/// A git repository opened for reading.
#[derive(Debug)]
pub struct Repository {
    path: PathBuf,
    git: gix::Repository,
    home: Option<PathBuf>,
}

/// A commit's tree, read from the object database.
#[derive(Debug)]
pub(crate) struct CommitTree<'r> {
    repository: &'r Repository,
    commit: ObjectId,
    root: ObjectId,
}

impl Repository {
    pub fn open(path: &Path) -> Result<Self, GitError> {
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
        Ok(Self {
            path,
            git,
            home: None,
        })
    }

    /// Sets the home directory that a mount's `~/` repository path is read
    /// against. The caller owns the environment, so Board reads it once.
    #[must_use]
    pub fn with_home(mut self, home: PathBuf) -> Self {
        self.home = Some(home);
        self
    }

    pub(crate) fn home(&self) -> Option<&Path> {
        self.home.as_deref()
    }

    /// The repository's canonical working directory, or its git directory
    /// when it has none.
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn git(&self) -> &gix::Repository {
        &self.git
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
                RawEntry {
                    name: entry.filename.to_vec(),
                    kind: match entry.mode.kind() {
                        EntryKind::Tree => Kind::Directory(id),
                        EntryKind::Blob | EntryKind::BlobExecutable => Kind::File(id),
                        EntryKind::Link => Kind::Symlink(id),
                        EntryKind::Commit => Kind::Submodule,
                    },
                }
            })
            .collect())
    }

    /// The bytes of blob `id`, or `None` when it holds more than
    /// [`MAX_RECORD_BYTES`]; the size is checked before the blob is loaded.
    fn read_blob(&self, id: ObjectId) -> Result<Option<Vec<u8>>, GitError> {
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

    fn source(&self) -> Source {
        Source::Commit(self.commit)
    }

    fn root(&self) -> ObjectId {
        self.root
    }

    fn list(&mut self, directory: &ObjectId) -> Result<Vec<RawEntry<ObjectId>>, GitError> {
        self.repository.list_tree(*directory)
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
            | Self::Object { .. } => Code::RepoUnreadable,
        }
    }
}
