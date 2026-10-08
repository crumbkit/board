//! What resolution and walking read: the directories of one source.

use std::rc::Rc;

use board_model::{Code, LogicalPath, ObjectId, Source, Stat};

use crate::address::{self, Named, Pointers};
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

    /// The entries of `directory` whose segment is `segment` or whose name
    /// is `name` but for ASCII case, in listing order: the only ones that
    /// bear on the code of an entry named `name` there.
    fn alike(
        &mut self,
        directory: &Self::Directory,
        name: &[u8],
        segment: Option<&str>,
    ) -> Result<Vec<RawEntry<Self::Directory>>, GitError> {
        let listed = self.list(directory)?;
        Ok(listed
            .iter()
            .filter(|entry| {
                entry.name.eq_ignore_ascii_case(name) || address::segment(entry) == segment
            })
            .cloned()
            .collect())
    }

    /// The bytes of the file `entry` in `directory`, or `None` when it holds
    /// more than [`board_model::MAX_RECORD_BYTES`].
    fn read(
        &mut self,
        directory: &Self::Directory,
        entry: &RawEntry<Self::Directory>,
    ) -> Result<Option<Vec<u8>>, GitError>;

    /// What `segment` names among `entries`, the listing of `directory`. A
    /// record beside its directory is read through `pointers` to tell
    /// whether it shadows the directory.
    fn named<'e>(
        &mut self,
        directory: &Self::Directory,
        entries: &'e [RawEntry<Self::Directory>],
        segment: &str,
        pointers: &mut Pointers,
    ) -> Result<Named<'e, Self::Directory>, GitError> {
        address::named(entries, segment, |record| {
            pointers.shadows(record, || self.read(directory, record))
        })
    }

    /// Each entry's logical path and own code in `directory`, whose logical
    /// path is `parent`, in the order of `entries`.
    fn address(
        &mut self,
        directory: &Self::Directory,
        parent: &LogicalPath,
        entries: &[RawEntry<Self::Directory>],
    ) -> Vec<(Option<LogicalPath>, Option<Code>)> {
        let mut pointers = Pointers::default();
        address::directory(parent, entries, |record| {
            pointers.shadows(record, || self.read(directory, record))
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
