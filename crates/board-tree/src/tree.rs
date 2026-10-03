//! What resolution and walking read: the directories of one source.

use board_model::{ObjectId, Source};

use crate::git::GitError;

/// One source's directory tree: a commit's, or the working tree.
pub(crate) trait Tree {
    /// How the source names a directory it can list.
    type Directory: Clone;

    fn source(&self) -> Source;

    fn root(&self) -> Self::Directory;

    /// The entries of `directory`, in no particular order.
    fn list(
        &mut self,
        directory: &Self::Directory,
    ) -> Result<Vec<RawEntry<Self::Directory>>, GitError>;

    /// The bytes of the file `entry` in `directory`, or `None` when it holds
    /// more than [`board_model::MAX_RECORD_BYTES`].
    fn read(
        &mut self,
        directory: &Self::Directory,
        entry: &RawEntry<Self::Directory>,
    ) -> Result<Option<Vec<u8>>, GitError>;
}

/// What an entry is, as git records it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Kind<D> {
    Directory(D),
    File(ObjectId),
    Symlink(ObjectId),
    Submodule,
    /// A file or symlink whose contents could not be read.
    Unreadable,
}

/// One entry of a directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RawEntry<D> {
    pub(crate) name: Vec<u8>,
    pub(crate) kind: Kind<D>,
}
