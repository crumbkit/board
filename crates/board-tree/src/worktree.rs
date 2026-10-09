//! The one module that reads the working tree: every file git sees, which is
//! tracked files and untracked files that are not ignored.

use std::cell::OnceCell;
use std::collections::HashMap;
use std::ffi::OsString;
use std::fs::{self, FileType, Metadata};
use std::io::{self, BufReader, Read};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::time::UNIX_EPOCH;

use board_model::{MAX_RECORD_BYTES, ObjectId, Role, Source, Stat};
use gix::bstr::{BString, ByteSlice};
use gix::index::entry::Mode;
use gix::worktree::stack::state::ignore;

use crate::address::{self, Named, Pointers};
use crate::git::{Contents, GitError, Repository, model_id};
use crate::tree::{Kind, RawEntry, Tree};

/// Given a file's `/`-separated path and metadata, its blob if already known.
pub type Known<'k> = &'k mut dyn FnMut(&str, &Stat) -> Option<ObjectId>;

/// Takes the contents of a file the walk read in full to hash it, given its
/// `/`-separated path.
pub type Kept<'k> = &'k mut dyn FnMut(&str, Contents);

/// The working tree of a repository, with its index and ignore rules. Each
/// directory is listed once, so the tree is one snapshot however often it is
/// read.
pub(crate) struct WorkingTree<'r, 'k> {
    repository: &'r Repository,
    index: gix::worktree::Index,
    excludes: gix::AttributeStack<'r>,
    /// Gives the blobs of files already known; without it, every file
    /// listed is hashed.
    known: Option<Known<'k>>,
    /// Takes the contents of each JSON file hashed, so the caller need not
    /// open it again; without it, they are dropped.
    kept: Option<Kept<'k>>,
    /// Whether a file is listed [`Kind::Unhashed`] and hashed only when
    /// [`Tree::settle`] is given it.
    on_demand: bool,
    listed: HashMap<PathBuf, Listing>,
    /// Each file settled, by path, so it keeps one blob in this snapshot.
    settled: HashMap<PathBuf, (Kind<PathBuf>, Option<Stat>)>,
}

/// One directory's entries, with the lookup made from them the first time a
/// name is looked up there.
struct Listing {
    entries: Rc<[RawEntry<PathBuf>]>,
    lookup: OnceCell<Lookup>,
}

/// Where each segment, and each name folded to lower case, is found in one
/// directory's entries, so finding a name does not scan a large directory.
#[derive(Default)]
struct Lookup {
    segments: HashMap<String, Vec<usize>>,
    folded: HashMap<Vec<u8>, Vec<usize>>,
}

impl Listing {
    /// The lookup for these entries, made on first use.
    fn lookup(&self) -> &Lookup {
        self.lookup.get_or_init(|| {
            let mut lookup = Lookup::default();
            for (at, entry) in self.entries.iter().enumerate() {
                if let Some(segment) = address::segment(entry) {
                    let found = lookup.segments.entry(segment.to_owned()).or_default();
                    found.push(at);
                }
                let folded = entry.name.to_ascii_lowercase();
                lookup.folded.entry(folded).or_default().push(at);
            }
            lookup
        })
    }

    /// The entries whose segment is `segment` or whose name is `name` but
    /// for ASCII case, in listing order.
    fn alike(&self, name: &[u8], segment: Option<&str>) -> Vec<RawEntry<PathBuf>> {
        let lookup = self.lookup();
        let by_segment = segment.and_then(|segment| lookup.segments.get(segment));
        let by_name = lookup.folded.get(&name.to_ascii_lowercase());
        let mut found: Vec<usize> = by_segment
            .into_iter()
            .chain(by_name)
            .flatten()
            .copied()
            .collect();
        found.sort_unstable();
        found.dedup();
        found
            .into_iter()
            .filter_map(|at| self.entries.get(at).cloned())
            .collect()
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

impl<'k> WorkingTree<'_, 'k> {
    /// This tree, handing the contents of each JSON file it hashes to `kept`.
    pub(crate) fn keeping(mut self, kept: Kept<'k>) -> Self {
        self.kept = Some(kept);
        self
    }

    /// This tree, hashing a file only when resolution picks or reads it.
    pub(crate) fn on_demand(mut self) -> Self {
        self.on_demand = true;
        self
    }

    /// What git sees at `relative`, with a file's or symlink's metadata, or
    /// `None` when git does not see it: an untracked path that is ignored,
    /// or something other than a file, directory or symlink. A path whose
    /// ignore rules or contents cannot be read is `Unreadable`.
    fn kind(
        &mut self,
        relative: &Path,
        file_type: FileType,
    ) -> Option<(Kind<PathBuf>, Option<Stat>)> {
        match ignored(
            &self.index,
            &mut self.excludes,
            relative,
            file_type.is_dir(),
        ) {
            Ok(true) => return None,
            Ok(false) => {}
            Err(_) => return Some((Kind::Unreadable, None)),
        }
        let absolute = self.repository.path().join(relative);
        if file_type.is_dir() {
            let kind = if submodule(&self.index, relative, &absolute) {
                Kind::Submodule
            } else {
                Kind::Directory(relative.to_path_buf())
            };
            return Some((kind, None));
        }
        let path = gix::path::into_bstr(relative).to_str_lossy().into_owned();
        if file_type.is_symlink() {
            let Ok(stat) = fs::symlink_metadata(&absolute).map(|metadata| stat(&metadata)) else {
                return Some((Kind::Unreadable, None));
            };
            let blob = self.known(&path, &stat).or_else(|| {
                let target = gix::path::into_bstr(fs::read_link(&absolute).ok()?);
                self.repository
                    .hash(&mut target.as_bytes(), target.len() as u64)
            });
            return Some((blob.map_or(Kind::Unreadable, Kind::Symlink), Some(stat)));
        }
        if !file_type.is_file() {
            return None;
        }
        if self.on_demand {
            return Some((Kind::Unhashed, None));
        }
        Some(self.file(relative, &path))
    }

    /// The file at `relative`, whose `/`-separated path is `path`: its blob
    /// and metadata, or `Unreadable`.
    fn file(&mut self, relative: &Path, path: &str) -> (Kind<PathBuf>, Option<Stat>) {
        let absolute = self.repository.path().join(relative);
        let Ok(seen) = fs::metadata(&absolute).map(|metadata| stat(&metadata)) else {
            return (Kind::Unreadable, None);
        };
        if let Some(blob) = self.known(path, &seen) {
            return (Kind::File(blob), Some(seen));
        }
        // The file hashed is the one opened, so its own metadata is kept.
        self.repository.opened_one();
        let Ok((stat, file)) =
            fs::File::open(&absolute).and_then(|file| Ok((stat(&file.metadata()?), file)))
        else {
            return (Kind::Unreadable, None);
        };
        // Only a walk that keeps bytes reads a file in full.
        let blob = match self.kept.as_mut() {
            Some(kept) if Role::of(path).is_some() && stat.size <= MAX_RECORD_BYTES as u64 => {
                hash_kept(self.repository, kept, path, file, stat.size)
            }
            _ => self.repository.hash(&mut BufReader::new(file), stat.size),
        };
        (blob.map_or(Kind::Unreadable, Kind::File), Some(stat))
    }

    /// The listing of `directory`, read from disk the first time.
    fn listing(&mut self, directory: &Path) -> Result<&Listing, GitError> {
        if !self.listed.contains_key(directory) {
            let listing = Listing {
                entries: self.read_directory(directory)?,
                lookup: OnceCell::new(),
            };
            self.listed.insert(directory.to_path_buf(), listing);
        }
        Ok(&self.listed[directory])
    }

    /// Reads the entries of `directory` from disk.
    fn read_directory(&mut self, directory: &Path) -> Result<Rc<[RawEntry<PathBuf>]>, GitError> {
        self.repository.listed_one();
        let absolute = self.repository.path().join(directory);
        let unreadable = |source| GitError::File {
            path: absolute.clone(),
            source,
        };
        let mut entries = Vec::new();
        for item in fs::read_dir(&absolute).map_err(unreadable)? {
            let item = item.map_err(unreadable)?;
            let name = item.file_name();
            if directory.as_os_str().is_empty() && name == ".git" {
                continue;
            }
            let kind = match item.file_type() {
                Ok(file_type) => self.kind(&directory.join(&name), file_type),
                Err(_) => Some((Kind::Unreadable, None)),
            };
            if let Some((kind, stat)) = kind {
                entries.push(RawEntry {
                    name: gix::path::into_bstr(Path::new(&name)).to_vec(),
                    kind,
                    stat,
                    size: stat.map(|stat| stat.size),
                });
            }
        }
        Ok(entries.into())
    }

    /// The blob of the file at `path` with `stat`, if already known.
    fn known(&mut self, path: &str, stat: &Stat) -> Option<ObjectId> {
        self.known.as_mut().and_then(|known| known(path, stat))
    }
}

/// The id of the `length` bytes of `file`, read in full and handed to `kept`,
/// so the blob is the blob of the bytes kept; `None` if the file cannot be
/// read or does not hold `length` bytes.
fn hash_kept(
    repository: &Repository,
    kept: &mut Kept<'_>,
    path: &str,
    file: fs::File,
    length: u64,
) -> Option<ObjectId> {
    let bytes = read_exactly(file, length)?;
    let blob = repository.blob_id(&bytes);
    kept(path, Contents { blob, bytes });
    Some(blob)
}

/// The bytes of `stream`, or `None` if it fails or yields fewer or more than
/// `length` bytes because the file changed while it was read.
fn read_exactly(stream: impl Read, length: u64) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    stream.take(length + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64 == length).then_some(bytes)
}

impl Tree for WorkingTree<'_, '_> {
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
        Ok(Rc::clone(&self.listing(directory)?.entries))
    }

    /// Finds the entries through the listing's lookup, so a write into a
    /// large directory addresses a few entries and not all of them.
    fn alike(
        &mut self,
        directory: &PathBuf,
        name: &[u8],
        segment: Option<&str>,
    ) -> Result<Vec<RawEntry<PathBuf>>, GitError> {
        Ok(self.listing(directory)?.alike(name, segment))
    }

    /// Uses the lookup only when `entries` is the listing of `directory` it
    /// was made from, and otherwise scans `entries`.
    fn named<'e>(
        &mut self,
        directory: &PathBuf,
        entries: &'e [RawEntry<PathBuf>],
        segment: &str,
        pointers: &mut Pointers,
    ) -> Result<Named<'e, PathBuf>, GitError> {
        let found = match self.listed.get(directory) {
            Some(listing) if std::ptr::eq(Rc::as_ptr(&listing.entries), entries) => Some(
                listing
                    .lookup()
                    .segments
                    .get(segment)
                    .cloned()
                    .unwrap_or_default(),
            ),
            _ => None,
        };
        // The record is settled first, so its parse is cached by blob.
        let shadows = |record: &RawEntry<PathBuf>| {
            let record = self.settle(directory, record);
            pointers.shadows(&record, || self.read(directory, &record))
        };
        match found {
            Some(found) => {
                address::classify(found.iter().filter_map(|at| entries.get(*at)), shadows)
            }
            None => address::named(entries, segment, shadows),
        }
    }

    /// Hashes an `Unhashed` file once; later calls give the same answer.
    fn settle(&mut self, directory: &PathBuf, entry: &RawEntry<PathBuf>) -> RawEntry<PathBuf> {
        if entry.kind != Kind::Unhashed {
            return entry.clone();
        }
        let relative = directory.join(gix::path::from_bstr(entry.name.as_bstr()));
        let (kind, stat) = if let Some(settled) = self.settled.get(&relative) {
            settled.clone()
        } else {
            let path = gix::path::into_bstr(&relative).to_str_lossy().into_owned();
            let settled = self.file(&relative, &path);
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
        let contents = self.repository.read_walked(&path, listed)?;
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

/// Whether the directory `relative`, at `absolute`, is a submodule: the
/// index holds a gitlink at its path, or it holds `.git`.
fn submodule(index: &gix::worktree::Index, relative: &Path, absolute: &Path) -> bool {
    let indexed = index.entry_by_path(key(relative).as_ref());
    indexed.is_some_and(|entry| entry.mode == Mode::COMMIT) || absolute.join(".git").exists()
}

/// The names in `directory`, spelled as the walk lists them.
fn names(directory: &Path) -> io::Result<Vec<OsString>> {
    fs::read_dir(directory)?
        .map(|item| item.map(|item| item.file_name()))
        .collect()
}

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
    /// target path, as git stores it. A file read that no longer holds the
    /// blob it was `walked` with is [`GitError::Changed`].
    pub fn read_working(
        &self,
        relative: impl AsRef<Path>,
        walked: Option<ObjectId>,
    ) -> Result<Working, GitError> {
        let path = self.path().join(relative);
        let unreadable = |source| GitError::File {
            path: path.clone(),
            source,
        };
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
        let contents = Contents {
            blob: self.blob_id(&bytes),
            bytes,
        };
        match walked {
            Some(blob) if blob != contents.blob => Err(GitError::Changed { path, contents }),
            _ => Ok(Working::Read(contents)),
        }
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
            if submodule(&index, &at, &absolute) {
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

    /// The id git gives a blob of `length` bytes read from `stream`, in
    /// constant memory; `None` if the stream fails, or yields fewer or more
    /// bytes than `length` because the file changed while it was read.
    fn hash(&self, stream: &mut dyn Read, length: u64) -> Option<ObjectId> {
        let id = gix::objs::compute_stream_hash(
            self.git().object_hash(),
            gix::objs::Kind::Blob,
            stream,
            length,
            &mut gix::features::progress::Discard,
            &AtomicBool::new(false),
        )
        .ok()?;
        let mut more = [0; 1];
        (stream.read(&mut more).ok()? == 0).then(|| model_id(&id))
    }

    /// The contents of a working-tree file that was walked, or `None` when it
    /// is too large. One that is gone is [`GitError::File`].
    pub(crate) fn read_walked(
        &self,
        relative: &Path,
        walked: Option<ObjectId>,
    ) -> Result<Option<Contents>, GitError> {
        match self.read_working(relative, walked)? {
            Working::Read(contents) => Ok(Some(contents)),
            Working::TooLarge => Ok(None),
            Working::Missing => Err(GitError::File {
                path: self.path().join(relative),
                source: io::ErrorKind::NotFound.into(),
            }),
        }
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
