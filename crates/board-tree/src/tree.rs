//! What resolution and walking read: the directories of one source.

use std::rc::Rc;

use board_model::{Code, LogicalPath, ObjectId, Source, Stat};

use crate::address;
use crate::git::GitError;

/// One source's directory tree: a commit's, or the working tree.
pub(crate) trait Tree {
    /// How the source names a directory it can list.
    type Directory: Clone;

    fn source(&self) -> Source;

    fn root(&self) -> Self::Directory;

    /// The entries of `directory`, in no particular order, shared so a
    /// listing can be kept and handed out again.
    fn list(
        &mut self,
        directory: &Self::Directory,
    ) -> Result<Rc<[RawEntry<Self::Directory>]>, GitError>;

    /// The bytes of the file `entry` in `directory`, or `None` when it holds
    /// more than [`board_model::MAX_RECORD_BYTES`].
    fn read(
        &mut self,
        directory: &Self::Directory,
        entry: &RawEntry<Self::Directory>,
    ) -> Result<Option<Vec<u8>>, GitError>;

    /// Each entry's logical path and own code in `directory`, whose logical
    /// path is `parent`, in the order of `entries`.
    fn address(
        &mut self,
        directory: &Self::Directory,
        parent: &LogicalPath,
        entries: &[RawEntry<Self::Directory>],
    ) -> Vec<(Option<LogicalPath>, Option<Code>)> {
        address::directory(parent, entries, |record| {
            // A record beside its directory is read only to learn whether it
            // is a pointer, which shadows the directory.
            match self.read(directory, record) {
                Ok(Some(bytes)) => Some(!matches!(board_model::pointer(&bytes), Ok(None))),
                Ok(None) => Some(false),
                Err(_) => None,
            }
        })
    }
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
    /// A working-tree file's or symlink's metadata.
    pub(crate) stat: Option<Stat>,
    /// A file's or symlink's size in bytes, when known.
    pub(crate) size: Option<u64>,
}
