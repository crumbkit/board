//! The one module that reads the working tree: every file git sees, which is
//! tracked files and untracked files that are not ignored.

mod list;
mod read;
mod settle;

pub use crate::worktree::read::{Directory, Reader, Working};

use std::cell::OnceCell;
use std::collections::HashMap;
use std::ffi::OsString;
use std::fs::{self, Metadata};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::UNIX_EPOCH;

use board_model::{ObjectId, Source, Stat};
use gix::bstr::{BString, ByteSlice};
use gix::index::entry::Mode;
use gix::worktree::stack::state::ignore;

use crate::address::{self, Named, Pointers};
use crate::git::{Contents, GitError, Repository};
use crate::tree::{Folded, Kind, RawEntry, Settled, Tree, Unhashed, Visit, is_alike};
use crate::worktree::list::{Item, Raw, RawKind};
use crate::worktree::settle::hash_stream;

/// Given a file's `/`-separated path and metadata, its blob if already known.
pub type Known<'k> = &'k mut dyn FnMut(&str, &Stat) -> Option<ObjectId>;

/// Makes what a walk hands to its `keep` from the contents of a file it
/// read in full to hash it, given its `/`-separated path. A walk hashes on
/// several threads, so it is called from any of them, and from more than
/// one at once.
pub type Prepare<'k, T> = &'k (dyn Fn(&str, Contents) -> T + Sync);

/// How many prepared files a walk holds for `keep` beyond one per hashing
/// thread, so the threads read ahead of a slow `keep` without holding the
/// whole tree. On synthetic-10k every bound from 4 to 1024 gave the same peak
/// memory (200 to 206 MB, against 279 to 283 MB unbounded) and the same time. 64 is
/// a middle value: it leaves room for a burst of small files, and it caps
/// what waits at 64 records even when each is near `MAX_RECORD_BYTES`.
pub(crate) const KEPT_AHEAD: usize = 64;

/// What a walk does with each file it reads in full.
struct Keeping<'k, T> {
    prepare: Prepare<'k, T>,
    /// Takes what `prepare` made, on the walking thread; `false` ends the
    /// walk.
    keep: &'k mut dyn FnMut(T) -> bool,
    /// Whether `keep` ended the walk, so its entries are incomplete.
    stopped: bool,
}

/// The working tree of a repository, with its index and ignore rules. Each
/// directory is listed once, so the tree is one snapshot however often it is
/// read.
pub(crate) struct WorkingTree<'r, 'k, T = ()> {
    repository: &'r Repository,
    index: gix::worktree::Index,
    excludes: gix::AttributeStack<'r>,
    /// Gives the blobs of files already known; without it, every file
    /// listed is hashed.
    known: Option<Known<'k>>,
    /// Prepares each JSON file hashed on the hashing threads and keeps it on
    /// the walking thread, so the caller need not open it again, and records
    /// whether `keep` ended the walk; without it, the contents are dropped.
    kept: Option<Keeping<'k, T>>,
    /// Whether a file is listed [`Kind::Unhashed`] and hashed only when
    /// [`Tree::settle`] is given it.
    on_demand: bool,
    listed: HashMap<PathBuf, Listing>,
    /// Each file settled, by path, so it keeps one blob in this snapshot.
    settled: HashMap<PathBuf, (Kind<PathBuf>, Option<Stat>)>,
}

/// One directory's items, each classified the first time a lookup returns
/// it, so that it keeps one answer in this snapshot.
struct Listing {
    items: Vec<Item>,
    /// Each item as git sees it, once classified: `None` when git does not
    /// see it.
    seen: Vec<OnceCell<Option<RawEntry<PathBuf>>>>,
    folded: Folded,
}

impl Listing {
    fn new(items: Vec<Item>) -> Self {
        Self {
            seen: items.iter().map(|_| OnceCell::new()).collect(),
            items,
            folded: Folded::default(),
        }
    }
}

impl Repository {
    /// The index, or an empty one when the repository has none.
    fn index(&self) -> Result<gix::worktree::Index, GitError> {
        self.git()
            .index_or_empty()
            .map_err(|error| GitError::Unreadable {
                repository: self.path().to_path_buf(),
                source: error.into(),
            })
    }

    /// The ignore rules of the working tree, as git reads them for `index`.
    fn excludes(&self, index: &gix::worktree::Index) -> Result<gix::AttributeStack<'_>, GitError> {
        self.git()
            .excludes(
                index,
                None,
                ignore::Source::WorktreeThenIdMappingIfNotSkipped,
            )
            .map_err(|error| GitError::Unreadable {
                repository: self.path().to_path_buf(),
                source: Box::new(error),
            })
    }

    pub(crate) fn working_tree<'k>(
        &self,
        known: Option<Known<'k>>,
    ) -> Result<WorkingTree<'_, 'k>, GitError> {
        if self.git().workdir().is_none() {
            return Err(GitError::NoWorkingTree {
                repository: self.path().to_path_buf(),
            });
        }
        let index = self.index()?;
        let excludes = self.excludes(&index)?;
        Ok(WorkingTree {
            repository: self,
            index,
            excludes,
            known,
            kept: None,
            on_demand: false,
            listed: HashMap::new(),
            settled: HashMap::new(),
        })
    }
}

impl<'r, 'k> WorkingTree<'r, 'k> {
    /// This tree, handing what `prepare` makes of each JSON file it hashes
    /// to `keep`, until `keep` answers `false`.
    pub(crate) fn keeping<T>(
        self,
        prepare: Prepare<'k, T>,
        keep: &'k mut dyn FnMut(T) -> bool,
    ) -> WorkingTree<'r, 'k, T> {
        WorkingTree {
            repository: self.repository,
            index: self.index,
            excludes: self.excludes,
            known: self.known,
            kept: Some(Keeping {
                prepare,
                keep,
                stopped: false,
            }),
            on_demand: self.on_demand,
            listed: self.listed,
            settled: self.settled,
        }
    }
}

impl<T: Send> WorkingTree<'_, '_, T> {
    /// This tree, hashing a file only when resolution picks or reads it.
    pub(crate) fn on_demand(mut self) -> Self {
        self.on_demand = true;
        self
    }

    /// Hashes the file at `relative` on this thread, handing what is
    /// prepared of it to `keep` unless `keep` has ended the walk.
    fn hash_one(
        &mut self,
        relative: &Path,
        path: &str,
        seen: Stat,
    ) -> (Kind<PathBuf>, Option<Stat>) {
        self.repository.opened_one();
        let (kind, stat, prepared) = self.hasher().hash(relative, path, seen);
        if let Some(keeping) = &mut self.kept
            && let Some(prepared) = prepared
            && !keeping.stopped
        {
            keeping.stopped = !(keeping.keep)(prepared);
        }
        (kind, stat)
    }

    /// What git sees at `relative`, listed as `raw`, with a file's or
    /// symlink's metadata, or `None` when git does not see it: an untracked
    /// path that is ignored. A path whose ignore rules or contents cannot be
    /// read is `Unreadable`.
    fn kind(&mut self, relative: &Path, raw: &Raw) -> Option<(Kind<PathBuf>, Option<Stat>)> {
        let is_dir = matches!(raw.kind, RawKind::Directory { .. });
        match ignored(&self.index, &mut self.excludes, relative, is_dir) {
            Ok(true) => return None,
            Ok(false) => {}
            Err(_) => return Some((Kind::Unreadable, None)),
        }
        let path = gix::path::into_bstr(relative).to_str_lossy().into_owned();
        Some(match &raw.kind {
            RawKind::Directory { has_git } => {
                let kind = if submodule(&self.index, relative, *has_git) {
                    Kind::Submodule
                } else {
                    Kind::Directory(relative.to_path_buf())
                };
                (kind, None)
            }
            // A symlink's text is in hand, and the blob the index knows for
            // it is the file's it leads to.
            RawKind::Symlink { stat, target } => {
                let blob = target
                    .as_deref()
                    .and_then(|target| self.repository.hash(&mut &target[..], target.len() as u64));
                (blob.map_or(Kind::Unreadable, Kind::Symlink), Some(*stat))
            }
            RawKind::File(seen) => match self.known(&path, seen) {
                Some(blob) => (Kind::File(blob), Some(*seen)),
                None if self.on_demand => (Kind::Unhashed(*seen), Some(*seen)),
                None => self.hash_one(relative, &path, *seen),
            },
            RawKind::Unreadable => (Kind::Unreadable, None),
        })
    }

    /// The listing of `directory`, read from disk the first time.
    fn listing(&mut self, directory: &Path) -> Result<&Listing, GitError> {
        if !self.listed.contains_key(directory) {
            let root = self.repository.path();
            let items =
                list::items(root, self.repository.listings(), directory).map_err(|source| {
                    GitError::File {
                        path: root.join(directory),
                        source,
                    }
                })?;
            self.listed
                .insert(directory.to_path_buf(), Listing::new(items));
        }
        Ok(&self.listed[directory])
    }

    /// Item `at` of the listing of `directory` as git sees it, classified
    /// the first time.
    fn entry(&mut self, directory: &Path, at: usize) -> Option<RawEntry<PathBuf>> {
        let listing = &self.listed[directory];
        if let Some(seen) = listing.seen[at].get() {
            return seen.clone();
        }
        let raw = list::raw(
            &self.repository.path().join(directory),
            listing.items[at].clone(),
        );
        let seen = self.seen(directory, raw);
        self.listed[directory].seen[at].get_or_init(|| seen).clone()
    }

    /// `raw`, an item of `directory`, as git sees it, or `None` when git
    /// does not see it.
    fn seen(&mut self, directory: &Path, raw: Raw) -> Option<RawEntry<PathBuf>> {
        let relative = directory.join(gix::path::from_bstr(raw.name.as_bstr()));
        let (kind, stat) = self.kind(&relative, &raw)?;
        Some(RawEntry {
            name: raw.name,
            kind,
            stat,
            size: stat.map(|stat| stat.size),
        })
    }

    /// The entries of `directory`, listed as `raw`, as git sees them, kept
    /// as its listing so that a later lookup there reads nothing again.
    fn classify(&mut self, directory: &Path, raw: Vec<Raw>) -> Rc<[RawEntry<PathBuf>]> {
        let (mut items, mut seen) = (Vec::new(), Vec::new());
        for raw in raw {
            // An item already classified is not read again, so its type
            // is not needed.
            let (name, file_type) = (raw.name.clone(), None);
            items.push(Item { name, file_type });
            seen.push(OnceCell::from(self.seen(directory, raw)));
        }
        let listed = seen.iter().filter_map(|seen| seen.get()?.clone()).collect();
        let listing = Listing {
            items,
            seen,
            folded: Folded::default(),
        };
        self.listed.insert(directory.to_path_buf(), listing);
        listed
    }

    /// The blob of the file at `path` with `stat`, if already known.
    fn known(&mut self, path: &str, stat: &Stat) -> Option<ObjectId> {
        self.known.as_mut().and_then(|known| known(path, stat))
    }
}

/// The bytes of `stream`, or `None` if it fails or yields fewer or more than
/// `length` bytes because the file changed while it was read.
fn read_exactly(stream: impl Read, length: u64) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    stream.take(length + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64 == length).then_some(bytes)
}

impl<T: Send> Tree for WorkingTree<'_, '_, T> {
    /// A directory's path relative to the working directory.
    type Directory = PathBuf;

    fn repository(&self) -> &Path {
        self.repository.path()
    }

    fn source(&self) -> Source {
        Source::WorkingTree
    }

    fn root(&self) -> PathBuf {
        PathBuf::new()
    }

    fn list(&mut self, directory: &PathBuf) -> Result<Rc<[RawEntry<PathBuf>]>, GitError> {
        let count = self.listing(directory)?.items.len();
        Ok((0..count)
            .filter_map(|at| self.entry(directory, at))
            .collect())
    }

    /// Classifies only the items whose names could give such an entry, so
    /// a lookup in a large directory classifies a few entries and not all
    /// of them.
    fn alike(
        &mut self,
        directory: &PathBuf,
        name: &[u8],
        segment: Option<&str>,
    ) -> Result<Vec<RawEntry<PathBuf>>, GitError> {
        let listing = self.listing(directory)?;
        let names = listing.items.iter().map(|item| item.name.as_slice());
        let found = listing.folded.find(names, name, segment);
        Ok(found
            .into_iter()
            .filter_map(|at| self.entry(directory, at))
            .filter(|entry| is_alike(entry, name, segment))
            .collect())
    }

    /// Settles a record beside its directory first, so its parse is cached
    /// by blob.
    fn named<'e>(
        &mut self,
        directory: &PathBuf,
        entries: &'e [RawEntry<PathBuf>],
        segment: &str,
        pointers: &mut Pointers,
    ) -> Result<Named<'e, PathBuf>, GitError> {
        address::named(entries, segment, |record| {
            let record = self.settle(directory, record);
            pointers.shadows(&record, || self.read(directory, &record))
        })
    }

    /// Hashes an `Unhashed` file once; later calls give the same answer.
    fn settle(&mut self, directory: &PathBuf, entry: &RawEntry<PathBuf>) -> RawEntry<PathBuf> {
        let Kind::Unhashed(seen) = &entry.kind else {
            return entry.clone();
        };
        let relative = directory.join(gix::path::from_bstr(entry.name.as_bstr()));
        let (kind, stat) = if let Some(settled) = self.settled.get(&relative) {
            settled.clone()
        } else {
            let path = gix::path::into_bstr(&relative).to_str_lossy().into_owned();
            let settled = self.hash_one(&relative, &path, *seen);
            self.settled.insert(relative, settled.clone());
            settled
        };
        RawEntry {
            name: entry.name.clone(),
            kind,
            stat,
            size: stat.map(|stat| stat.size),
        }
    }

    fn settle_all(&mut self, unhashed: Vec<Unhashed<PathBuf>>) -> Vec<Settled<PathBuf>> {
        self.settle_each(unhashed)
    }

    /// Lists ahead on the repository's threads.
    fn visit_all<P: Send>(
        &mut self,
        start: (PathBuf, P),
        visit: Visit<'_, Self, P>,
    ) -> Result<(), GitError> {
        self.visit_ahead(start, visit)
    }

    fn read(
        &mut self,
        directory: &PathBuf,
        entry: &RawEntry<PathBuf>,
    ) -> Result<Option<Vec<u8>>, GitError> {
        let entry = &self.settle(directory, entry);
        let path = directory.join(gix::path::from_bstr(entry.name.as_bstr()));
        let listed = match entry.kind {
            Kind::File(blob) | Kind::Symlink(blob) => Some(blob),
            _ => None,
        };
        // The entry's own bytes, a symlink's text: a symlink is followed
        // only by whoever asked for it.
        let working = self.repository.read_working(&path)?;
        let contents = self.repository.as_walked(&path, working, listed)?;
        Ok(contents.map(|contents| contents.bytes))
    }
}

/// A file's metadata. Where the platform has no inode, change time or mode,
/// each is 0: a matching stat only lets hashing be skipped, and the
/// racy-clean rule still hashes anything modified recently.
fn stat(metadata: &Metadata) -> Stat {
    let modified_ns = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .and_then(|since| i64::try_from(since.as_nanos()).ok())
        .unwrap_or(0);
    let (changed_ns, inode, mode) = unix_stat(metadata);
    Stat {
        size: metadata.len(),
        modified_ns,
        changed_ns,
        inode,
        mode,
    }
}

/// The change time in nanoseconds, the inode and the mode.
#[cfg(unix)]
fn unix_stat(metadata: &Metadata) -> (i64, u64, u32) {
    use std::os::unix::fs::MetadataExt;
    let changed_ns = metadata
        .ctime()
        .saturating_mul(1_000_000_000)
        .saturating_add(metadata.ctime_nsec());
    (changed_ns, metadata.ino(), metadata.mode())
}

#[cfg(not(unix))]
fn unix_stat(_metadata: &Metadata) -> (i64, u64, u32) {
    (0, 0, 0)
}

/// `relative` as a key into the index.
fn key(relative: &Path) -> std::borrow::Cow<'_, gix::bstr::BStr> {
    gix::path::to_unix_separators_on_windows(gix::path::into_bstr(relative))
}

/// Whether the walk leaves out `relative`, a directory when `is_dir`: git
/// sees a tracked path whatever the ignore rules say, and an untracked one
/// unless they ignore it.
fn ignored(
    index: &gix::worktree::Index,
    excludes: &mut gix::AttributeStack<'_>,
    relative: &Path,
    is_dir: bool,
) -> io::Result<bool> {
    let key = key(relative);
    let tracked = index.entry_by_path(key.as_ref()).is_some()
        || is_dir && {
            let mut prefix = BString::from(key.as_bytes());
            prefix.push(b'/');
            index.prefixed_entries(prefix.as_ref()).is_some()
        };
    if tracked {
        return Ok(false);
    }
    let mode = is_dir.then_some(Mode::DIR);
    Ok(excludes.at_path(relative, mode)?.is_excluded())
}

/// Whether the directory `relative` is a submodule: the index holds a
/// gitlink at its path, or it `has_git`, holding `.git`.
fn submodule(index: &gix::worktree::Index, relative: &Path, has_git: bool) -> bool {
    let indexed = index.entry_by_path(key(relative).as_ref());
    indexed.is_some_and(|entry| entry.mode == Mode::COMMIT) || has_git
}

/// The names in `directory`, spelled as the walk lists them.
fn names(directory: &Path) -> io::Result<Vec<OsString>> {
    fs::read_dir(directory)?
        .map(|item| item.map(|item| item.file_name()))
        .collect()
}

impl Repository {
    /// The id git gives a blob of `length` bytes read from `stream`, in
    /// constant memory; `None` if the stream fails, or yields fewer or more
    /// bytes than `length` because the file changed while it was read.
    fn hash(&self, stream: &mut dyn Read, length: u64) -> Option<ObjectId> {
        hash_stream(self.git().object_hash(), stream, length)
    }
}

/// Whether `directory` is the root of a repository's working tree: it holds
/// `.git`, as a directory or as the file a linked worktree has.
pub fn is_repository_root(directory: &Path) -> bool {
    directory.join(".git").exists()
}

/// The root of the git working tree holding `start`, or `start` itself,
/// searching upward as git does.
pub fn repository_root(start: &Path) -> Option<PathBuf> {
    let start = start.canonicalize().ok()?;
    start
        .ancestors()
        .find(|directory| is_repository_root(directory))
        .map(Path::to_path_buf)
}

/// The directory holding `.board/board.json` nearest above `start`, or
/// `start` itself, searching upward as git does. Each name must be listed in
/// that exact spelling, as the walk requires, so a case-insensitive file
/// system finds no board at `.Board/board.json`.
pub fn discover(start: &Path) -> Option<PathBuf> {
    let start = start.canonicalize().ok()?;
    start
        .ancestors()
        .find(|directory| holds_config(directory))
        .map(Path::to_path_buf)
}

fn holds_config(directory: &Path) -> bool {
    let board = directory.join(".board");
    board.join("board.json").is_file() && lists(directory, ".board") && lists(&board, "board.json")
}

/// Whether `directory` lists `name` in that exact spelling.
fn lists(directory: &Path, name: &str) -> bool {
    // An unlistable directory stops discovery here, so the walk reports it.
    names(directory).map_or(true, |names| names.iter().any(|listed| listed == name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::model_id;

    #[test]
    fn a_stream_longer_or_shorter_than_its_length_has_no_id() {
        let directory = tempfile::tempdir().unwrap();
        gix::init(directory.path()).unwrap();
        let repository = Repository::open(directory.path()).unwrap();
        let expected =
            gix::objs::compute_hash(gix::hash::Kind::Sha1, gix::objs::Kind::Blob, b"abc").unwrap();
        assert_eq!(
            repository.hash(&mut &b"abc"[..], 3),
            Some(model_id(&expected))
        );
        assert_eq!(repository.hash(&mut &b"abcd"[..], 3), None);
        assert_eq!(repository.hash(&mut &b"ab"[..], 3), None);
    }

    #[test]
    fn a_stream_longer_or_shorter_than_its_length_is_not_read() {
        assert_eq!(read_exactly(&b"abc"[..], 3), Some(b"abc".to_vec()));
        assert_eq!(read_exactly(&b"abcd"[..], 3), None);
        assert_eq!(read_exactly(&b"ab"[..], 3), None);
    }

    #[test]
    fn a_name_is_listed_only_in_its_own_spelling() {
        for (written, listed) in [(".board", true), (".Board", false)] {
            let scenario = board_scenario::Scenario::new();
            scenario.write(&format!("{written}/x.md"), board_scenario::Item::File(""));
            assert_eq!(lists(scenario.workdir(), ".board"), listed, "{written}");
        }
    }

    #[test]
    fn a_name_is_found_in_the_entries_given_even_when_they_are_not_the_listing() {
        let scenario = board_scenario::Scenario::new();
        let empty = board_scenario::Item::File("{}");
        scenario.write("a.json", empty).write("b.json", empty);
        let repository = Repository::open(scenario.workdir()).unwrap();
        let mut tree = repository.working_tree(None).unwrap();
        let root = PathBuf::new();
        let listed = tree.list(&root).unwrap();
        let reversed: Vec<RawEntry<PathBuf>> = listed.iter().rev().cloned().collect();
        for entries in [&listed[..], &reversed[..]] {
            let Named::One(found) = tree
                .named(&root, entries, "a", &mut Pointers::default())
                .unwrap()
            else {
                panic!("a.json is the one entry named a");
            };
            assert_eq!(found.name, b"a.json");
        }
    }
}
