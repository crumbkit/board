//! What resolution and walking read: the directories of one source.

use std::cell::OnceCell;
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

use board_model::{Code, LogicalPath, ObjectId, Source, Stat};

use crate::address::{self, Named, Pointers};
use crate::git::GitError;

/// One source's directory tree: a commit's, or the working tree.
pub(crate) trait Tree {
    /// How the source names a directory it can list.
    type Directory: Clone + Send;

    /// The path of the repository the source is in, as
    /// [`crate::Repository::path`] gives it.
    fn repository(&self) -> &Path;

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
            .filter(|entry| is_alike(entry, name, segment))
            .cloned()
            .collect())
    }

    /// The bytes of the file `entry` in `directory`, or `None` when it holds
    /// more than [`board_model::MAX_RECORD_BYTES`]. A symlink's bytes are
    /// its text.
    fn read(
        &mut self,
        directory: &Self::Directory,
        entry: &RawEntry<Self::Directory>,
    ) -> Result<Option<Vec<u8>>, GitError>;

    /// `entry` of `directory` with its blob, for a source that lists a file
    /// before hashing it.
    fn settle(
        &mut self,
        _directory: &Self::Directory,
        entry: &RawEntry<Self::Directory>,
    ) -> RawEntry<Self::Directory> {
        entry.clone()
    }

    /// The blob and metadata of each file a walk listed `Unhashed`, in any
    /// order. A source that hashes every file it lists has none.
    fn settle_all(
        &mut self,
        _unhashed: Vec<Unhashed<Self::Directory>>,
    ) -> Vec<Settled<Self::Directory>> {
        vec![]
    }

    /// Gives `visit` the listing of `start`, then of each directory `visit`
    /// returns, each with the payload it was returned with, in any order.
    /// A commit's tree lists one directory at a time; the working tree
    /// lists ahead of `visit` on the repository's threads.
    fn visit_all<P: Send>(
        &mut self,
        start: (Self::Directory, P),
        visit: Visit<'_, Self, P>,
    ) -> Result<(), GitError>
    where
        Self: Sized,
    {
        visit_each(self, start, visit)
    }

    /// What `segment` names among `entries` of `directory`. A
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

/// The listing of a directory, or why it could not be listed.
pub(crate) type Listed<D> = Result<Rc<[RawEntry<D>]>, GitError>;

/// What [`Tree::visit_all`] calls with each directory, its payload and its
/// listing: it returns the directories to list next, each with a payload.
pub(crate) type Visit<'v, T, P> =
    &'v mut dyn FnMut(
        &mut T,
        (<T as Tree>::Directory, P),
        Listed<<T as Tree>::Directory>,
    ) -> Result<Vec<(<T as Tree>::Directory, P)>, GitError>;

/// Lists `start` and what `visit` returns one directory at a time, on this
/// thread, deepest first.
pub(crate) fn visit_each<T: Tree, P>(
    tree: &mut T,
    start: (T::Directory, P),
    visit: Visit<'_, T, P>,
) -> Result<(), GitError> {
    let mut pending = vec![start];
    while let Some((directory, payload)) = pending.pop() {
        let listed = tree.list(&directory);
        pending.extend(visit(tree, (directory, payload), listed)?);
    }
    Ok(())
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
    /// A working-tree file not hashed yet, with the metadata it was seen
    /// with, which [`Tree::settle`] hashes.
    Unhashed(Stat),
}

/// A file a walk listed without hashing: which of the walk's entries it is,
/// the directory and name that find it, and the metadata it was seen with.
#[derive(Debug)]
pub(crate) struct Unhashed<D> {
    pub(crate) at: usize,
    pub(crate) directory: D,
    pub(crate) name: Vec<u8>,
    pub(crate) seen: Stat,
}

/// What hashing an [`Unhashed`] file found: which entry it is, its kind and
/// its metadata.
pub(crate) type Settled<D> = (usize, Kind<D>, Option<Stat>);

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

/// Whether `entry` bears on the code of an entry named `name`, whose segment
/// is `segment`, beside it: its segment is `segment`, or its name is `name`
/// but for ASCII case.
pub(crate) fn is_alike<D>(entry: &RawEntry<D>, name: &[u8], segment: Option<&str>) -> bool {
    entry.name.eq_ignore_ascii_case(name) || address::segment(entry) == segment
}

/// Where each name of one listing, folded to ASCII lower case, is found in
/// it, made the first time a name is looked up there, so that a lookup does
/// not scan a large directory.
#[derive(Debug, Default)]
pub(crate) struct Folded(OnceCell<HashMap<Vec<u8>, Vec<usize>>>);

impl Folded {
    /// Where the items of the listing whose names are `names` may be alike
    /// to `name` and `segment`, in listing order: those named `name`,
    /// `segment` or `segment.json` but for ASCII case.
    pub(crate) fn find<'n>(
        &self,
        names: impl Iterator<Item = &'n [u8]>,
        name: &[u8],
        segment: Option<&str>,
    ) -> Vec<usize> {
        let folded = self.0.get_or_init(|| {
            let mut folded: HashMap<Vec<u8>, Vec<usize>> = HashMap::new();
            for (at, name) in names.enumerate() {
                folded
                    .entry(name.to_ascii_lowercase())
                    .or_default()
                    .push(at);
            }
            folded
        });
        let record = segment.map(|segment| format!("{segment}.json"));
        let wanted = [
            Some(name),
            segment.map(str::as_bytes),
            record.as_deref().map(str::as_bytes),
        ];
        let mut found: Vec<usize> = wanted
            .into_iter()
            .flatten()
            .filter_map(|name| folded.get(&name.to_ascii_lowercase()))
            .flatten()
            .copied()
            .collect();
        found.sort_unstable();
        found.dedup();
        found
    }
}

/// A directory's entries, with their names folded for [`Tree::alike`].
#[derive(Debug)]
pub(crate) struct Indexed<D> {
    pub(crate) entries: Rc<[RawEntry<D>]>,
    folded: Folded,
}

impl<D: Clone> Indexed<D> {
    pub(crate) fn new(entries: Rc<[RawEntry<D>]>) -> Self {
        Self {
            entries,
            folded: Folded::default(),
        }
    }

    /// The entries alike to `name` and `segment`, as [`Tree::alike`] gives
    /// them.
    pub(crate) fn alike(&self, name: &[u8], segment: Option<&str>) -> Vec<RawEntry<D>> {
        let names = self.entries.iter().map(|entry| entry.name.as_slice());
        self.folded
            .find(names, name, segment)
            .into_iter()
            .map(|at| &self.entries[at])
            .filter(|entry| is_alike(entry, name, segment))
            .cloned()
            .collect()
    }
}
