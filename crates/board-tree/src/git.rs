//! The one module that reads git objects and references.

use std::collections::HashMap;
use std::error::Error;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

use board_model::{Code, Fields, LogicalPath, MAX_RECORD_BYTES, ObjectId, Source};
use gix::object::tree::EntryKind;

use crate::symlink::Refused;
use crate::tree::{Indexed, Kind, RawEntry, Tree};

/// The most threads a walk lists directories and hashes files on unless the
/// repository is set to another number. Measured on an M4 Max: listing a
/// 10,000-record board took 45 ms on one thread, 20 on three and 17 on
/// four; hashing a 10,000-record board of
/// 7 KB files took 398 ms on one thread, 150 ms on four and 125 ms on six,
/// while a board of 90-byte files took 83 ms on three threads, 110 ms on four
/// and 260 ms on eight, because opening small files contends in the kernel.
const MAX_THREADS: usize = 4;

/// The cores available, up to [`MAX_THREADS`], asked of the system once:
/// on Linux the question reads cgroup files.
fn default_threads() -> usize {
    static THREADS: OnceLock<usize> = OnceLock::new();
    *THREADS.get_or_init(|| {
        std::thread::available_parallelism()
            .map_or(1, NonZeroUsize::get)
            .min(MAX_THREADS)
    })
}

/// A git repository opened for reading.
#[derive(Debug)]
pub struct Repository {
    path: PathBuf,
    git: gix::Repository,
    /// How many directories its working-tree views read from disk.
    listed: AtomicUsize,
    /// How many working-tree files it opened to hash or read.
    opened: AtomicUsize,
    /// What it read from the object database, shared by every `Repository`
    /// that [`Repositories`](crate::Repositories) lends for it.
    reads: Arc<ObjectReads>,
    /// How many threads a walk lists directories and hashes files on.
    threads: AtomicUsize,
}

/// What a repository read from its object database. Work-count tests
/// assert it.
#[derive(Debug, Default)]
pub(crate) struct ObjectReads {
    trees: AtomicUsize,
    sizes: AtomicUsize,
}

/// Which entries of a tree get their size from the object database.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Sizes {
    /// Every file and symlink, as a walk reports them.
    All,
    /// Symlinks only: resolution needs a symlink's size to tell whether to
    /// read its text, and no file's.
    Symlinks,
}

/// A commit's tree, read from the object database. Each directory is
/// decoded once however often it is listed.
#[derive(Debug)]
pub(crate) struct CommitTree<'r> {
    repository: &'r Repository,
    commit: ObjectId,
    root: ObjectId,
    listed: HashMap<ObjectId, Indexed<ObjectId>>,
    sizes: Sizes,
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
        Ok(Self::lent(path, git, Arc::default()))
    }

    /// The repository at the canonical `path` that `git` has open.
    pub(crate) fn lent(path: PathBuf, git: gix::Repository, reads: Arc<ObjectReads>) -> Self {
        Self {
            path,
            git,
            listed: AtomicUsize::new(0),
            opened: AtomicUsize::new(0),
            reads,
            threads: AtomicUsize::new(default_threads()),
        }
    }

    /// How many threads a walk of the working tree lists directories and
    /// hashes files on: the cores available, up to four, unless
    /// [`Repository::set_threads`] said otherwise.
    pub(crate) fn threads(&self) -> usize {
        self.threads.load(Ordering::Relaxed)
    }

    /// Makes each walk of the working tree list and hash on `threads`
    /// threads. Determinism tests walk with one thread and with several.
    pub fn set_threads(&self, threads: NonZeroUsize) {
        self.threads.store(threads.get(), Ordering::Relaxed);
    }

    /// The repository's canonical working directory, or its git directory
    /// when it has none.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Where `file`, a working-tree path, lands on disk: its deepest
    /// ancestor that exists, as the file system spells it, then the rest as
    /// given. On a case-insensitive file system another spelling of an
    /// existing directory lands in it, and this names that directory. A
    /// path whose ancestor resolves outside the repository is kept as given.
    pub fn landing(&self, file: &str) -> String {
        let mut existing = self.path.join(file);
        let mut rest = Vec::new();
        while existing.starts_with(&self.path) {
            if let Ok(canonical) = existing.canonicalize() {
                let Ok(inside) = canonical.strip_prefix(&self.path) else {
                    break;
                };
                let inside = inside
                    .iter()
                    .map(|part| part.to_string_lossy().into_owned());
                return inside
                    .chain(rest.into_iter().rev())
                    .collect::<Vec<_>>()
                    .join("/");
            }
            let Some(name) = existing.file_name() else {
                break;
            };
            rest.push(name.to_string_lossy().into_owned());
            existing.pop();
        }
        file.to_owned()
    }

    pub(crate) fn git(&self) -> &gix::Repository {
        &self.git
    }

    /// How many directories this repository's working-tree views have read
    /// from disk. Work-count tests assert it.
    pub fn directories_listed(&self) -> usize {
        self.listed.load(Ordering::Relaxed)
    }

    /// The count of directories read from disk, for a listing to add to.
    pub(crate) fn listings(&self) -> &AtomicUsize {
        &self.listed
    }

    /// How many working-tree files this repository opened to hash or read.
    /// Work-count tests assert it.
    pub fn files_opened(&self) -> usize {
        self.opened.load(Ordering::Relaxed)
    }

    /// The counts of what was read from the object database, for each
    /// `Repository` lent for this one to add to.
    pub(crate) fn reads(&self) -> Arc<ObjectReads> {
        Arc::clone(&self.reads)
    }

    /// How many trees this repository decoded from the object database,
    /// through this `Repository` or any other lent for it. Work-count tests
    /// assert it.
    pub fn trees_decoded(&self) -> usize {
        self.reads.trees.load(Ordering::Relaxed)
    }

    /// How many blob sizes this repository read from the object database,
    /// counted as [`Repository::trees_decoded`] is.
    pub fn sizes_read(&self) -> usize {
        self.reads.sizes.load(Ordering::Relaxed)
    }

    /// Counts one working-tree file opened.
    pub(crate) fn opened_one(&self) {
        self.opened_many(1);
    }

    /// Counts `count` working-tree files opened.
    pub(crate) fn opened_many(&self, count: usize) {
        self.opened.fetch_add(count, Ordering::Relaxed);
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
            listed: HashMap::new(),
            sizes: Sizes::All,
        })
    }

    /// The entries of `tree`, in git's order.
    fn list_tree(&self, tree: ObjectId, sizes: Sizes) -> Result<Vec<RawEntry<ObjectId>>, GitError> {
        Ok(self
            .tree_entries(tree, sizes)?
            .into_iter()
            .map(|(entry, _, _)| entry)
            .collect())
    }

    /// The entries of `tree`, in git's order, each with git's mode and the
    /// object it names, and the size `sizes` asks for.
    pub(crate) fn tree_entries(
        &self,
        tree: ObjectId,
        sizes: Sizes,
    ) -> Result<Vec<(RawEntry<ObjectId>, u32, ObjectId)>, GitError> {
        let found = self
            .git
            .find_tree(self.gix_id(tree)?)
            .map_err(|error| self.missing(tree, error))?;
        let decoded = found.decode().map_err(|error| self.missing(tree, error))?;
        self.reads.trees.fetch_add(1, Ordering::Relaxed);
        Ok(decoded
            .entries
            .iter()
            .map(|entry| {
                let id = model_id(entry.oid);
                let wanted = matches!(
                    (entry.mode.kind(), sizes),
                    (EntryKind::Link, _)
                        | (EntryKind::Blob | EntryKind::BlobExecutable, Sizes::All)
                );
                let size = wanted
                    .then(|| {
                        self.reads.sizes.fetch_add(1, Ordering::Relaxed);
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

    pub(crate) fn gix_id(&self, id: ObjectId) -> Result<gix::ObjectId, GitError> {
        gix::ObjectId::try_from(id.as_bytes()).map_err(|error| self.missing(id, error))
    }

    pub(crate) fn missing(
        &self,
        id: ObjectId,
        source: impl Into<Box<dyn Error + Send + Sync>>,
    ) -> GitError {
        GitError::Object {
            repository: self.path.clone(),
            id,
            source: source.into(),
        }
    }
}

impl CommitTree<'_> {
    /// This tree, giving the entries it lists the sizes `sizes` asks for.
    pub(crate) fn sizing(mut self, sizes: Sizes) -> Self {
        self.sizes = sizes;
        self
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
        if let Some(listed) = self.listed.get(directory) {
            return Ok(Rc::clone(&listed.entries));
        }
        let listed: Rc<[_]> = self.repository.list_tree(*directory, self.sizes)?.into();
        self.listed
            .insert(*directory, Indexed::new(Rc::clone(&listed)));
        Ok(listed)
    }

    fn alike(
        &mut self,
        directory: &ObjectId,
        name: &[u8],
        segment: Option<&str>,
    ) -> Result<Vec<RawEntry<ObjectId>>, GitError> {
        self.list(directory)?;
        Ok(self.listed[directory].alike(name, segment))
    }

    fn read(
        &mut self,
        _directory: &ObjectId,
        entry: &RawEntry<ObjectId>,
    ) -> Result<Option<Vec<u8>>, GitError> {
        match entry.kind {
            Kind::File(id) | Kind::Symlink(id) => self.repository.read_blob(id),
            _ => Ok(None),
        }
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
    /// The working-tree symlink at `path` in the repository leads to no
    /// file.
    #[error("/{} is {refused}", path.display())]
    Symlink { path: PathBuf, refused: Refused },
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
            Self::Symlink { refused, .. } => refused.code,
        }
    }

    /// A symlink's board path and text, which its hint names.
    pub fn fields(&self) -> Fields {
        match self {
            Self::Symlink { path, refused } => Fields {
                path: LogicalPath::root().join(&path.to_string_lossy()).ok(),
                target: Some(refused.target.clone()),
                ..Fields::default()
            },
            _ => Fields::default(),
        }
    }
}
