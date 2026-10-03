//! The one module that reads git objects.

use std::error::Error;
use std::path::{Path, PathBuf};

use board_model::{Code, ObjectId};
use gix::object::tree::EntryKind;

/// A git repository opened for reading.
#[derive(Debug)]
pub struct Repository {
    path: PathBuf,
    git: gix::Repository,
}

/// What a tree entry is, as git records it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Directory,
    File,
    Symlink,
    Submodule,
}

/// One entry of a git tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RawEntry {
    pub(crate) name: Vec<u8>,
    pub(crate) kind: Kind,
    pub(crate) id: ObjectId,
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
        Ok(Self { path, git })
    }

    /// The repository's canonical working directory, or its git directory
    /// when it has none.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The root tree of `commit`.
    pub(crate) fn root_tree(&self, commit: ObjectId) -> Result<ObjectId, GitError> {
        let found = self
            .git
            .find_commit(self.gix_id(commit)?)
            .map_err(|error| self.missing(commit, error))?;
        let tree = found
            .tree_id()
            .map_err(|error| self.missing(commit, error))?;
        Ok(model_id(&tree))
    }

    /// The entries of `tree`, in git's order.
    pub(crate) fn list(&self, tree: ObjectId) -> Result<Vec<RawEntry>, GitError> {
        let found = self
            .git
            .find_tree(self.gix_id(tree)?)
            .map_err(|error| self.missing(tree, error))?;
        let decoded = found.decode().map_err(|error| self.missing(tree, error))?;
        Ok(decoded
            .entries
            .iter()
            .map(|entry| RawEntry {
                name: entry.filename.to_vec(),
                kind: match entry.mode.kind() {
                    EntryKind::Tree => Kind::Directory,
                    EntryKind::Blob | EntryKind::BlobExecutable => Kind::File,
                    EntryKind::Link => Kind::Symlink,
                    EntryKind::Commit => Kind::Submodule,
                },
                id: model_id(entry.oid),
            })
            .collect())
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

fn model_id(id: &gix::oid) -> ObjectId {
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
            Self::Unreadable { .. } | Self::Object { .. } => Code::TreeUnreadable,
        }
    }
}
