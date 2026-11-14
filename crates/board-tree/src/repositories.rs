//! The board's repository and every repository its mounts name, each opened
//! once however many pointers and Locations name it.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use board_model::{Board, Location, Mount, Moved, ObjectId, Source};

use crate::git::{Contents, GitError, ObjectReads, Repository};
use crate::worktree::Reader;
use crate::write::{Lock, WriteError};

/// The repository a board is in, and a cache of every repository its mounts
/// name, so each is opened from disk once. Each lookup gives a
/// [`Repository`] of its own that reads from the cached object database.
/// `Repositories` is not `Sync`, because the root `Repository` is not.
#[derive(Debug)]
pub struct Repositories {
    root: Repository,
    /// What a mount's `~/` repository path is read against. The caller owns
    /// the environment, so Board reads it once.
    home: Option<PathBuf>,
    /// Each repository opened so far, under every canonical path that opens
    /// it: see [`Opened::insert`].
    open: Mutex<HashMap<PathBuf, Opened>>,
    /// How many repositories the cache has opened from disk.
    opened: AtomicUsize,
    /// The working tree's mount pointer read at another commit for this
    /// run: see [`Repositories::read_at`].
    checkout: Option<Checkout>,
    /// The moved prefixes a resolution that names nothing tries: see
    /// [`Repositories::moving`].
    pub(crate) moved: Vec<Moved>,
    /// What the board's `min_board` makes of writes: see
    /// [`Repositories::gate_writes`].
    gate: Option<WriteGate>,
}

std::thread_local! {
    /// The notes of the writes this thread admitted past a
    /// [`WriteGate::Unchecked`] gate, from every board it wrote, whichever
    /// [`Repositories`] held the gate.
    static UNCHECKED: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Takes the notes of the writes this thread admitted without checking a
/// build date, because the build has none, from every board it wrote since
/// the last call. Each note is there once.
pub fn take_unchecked_notes() -> Vec<String> {
    UNCHECKED.with(|notes| std::mem::take(&mut *notes.borrow_mut()))
}

/// What a board's `min_board` makes of this Board's writes, when it makes
/// anything (docs/architecture/writes/minimum.md).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WriteGate {
    /// This Board is older than `min_board` allows: every write and sync is
    /// refused with `board::config::board_too_old`. `install` says what to
    /// install to satisfy it.
    TooOld {
        min_board: String,
        running: String,
        install: String,
    },
    /// `min_board` is a value this Board cannot read, so it cannot tell
    /// whether it is old enough: every write and sync is refused with
    /// `board::config::min_board_unreadable`.
    Unreadable { detail: String },
    /// `min_board` names a build date and this build has none, so the date
    /// cannot be checked: writes are admitted, and
    /// [`take_unchecked_notes`] says so. Holds the text of the note to show.
    Unchecked { note: String },
}

/// A mount pointer in the working tree that one run reads at a checkout's
/// HEAD instead of the commit it pins (`board --checkout`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Checkout {
    /// The pointer's physical path in the working tree, such as
    /// `repos/lib.json`.
    pub pointer: String,
    /// The commit the run reads the mount at.
    pub commit: ObjectId,
}

/// A repository in the cache, and the path [`Repository::path`] gives it.
#[derive(Clone, Debug)]
struct Opened {
    path: PathBuf,
    git: gix::ThreadSafeRepository,
    reads: Arc<ObjectReads>,
}

impl Opened {
    fn of(repository: &Repository) -> Self {
        Self {
            path: repository.path().to_path_buf(),
            git: repository.git().clone().into_sync(),
            reads: repository.reads(),
        }
    }

    /// Adds this repository to `open` under each canonical path that opens
    /// it: its working tree, its git directory, and its working tree's
    /// `.git`, which in a linked worktree is a file naming the git directory.
    fn insert(self, open: &mut HashMap<PathBuf, Opened>) {
        let git_dir = self.git.git_dir().canonicalize();
        let dot_git = self.path.join(".git").canonicalize();
        for key in [git_dir, dot_git].into_iter().flatten() {
            open.insert(key, self.clone());
        }
        open.insert(self.path.clone(), self);
    }

    fn lend(&self) -> Repository {
        Repository::lent(
            self.path.clone(),
            self.git.to_thread_local(),
            Arc::clone(&self.reads),
        )
    }
}

impl Repositories {
    /// Opens the repository at `root`. `home` is what a mount's `~/` means.
    pub fn open(root: &Path, home: Option<PathBuf>) -> Result<Self, GitError> {
        let root = Repository::open(root)?;
        let mut open = HashMap::new();
        Opened::of(&root).insert(&mut open);
        Ok(Self {
            root,
            home,
            open: Mutex::new(open),
            opened: AtomicUsize::new(0),
            checkout: None,
            moved: Vec::new(),
            gate: None,
        })
    }

    pub fn root(&self) -> &Repository {
        &self.root
    }

    /// Reads the mount pointer `checkout` names in the working tree at its
    /// commit, instead of the commit it pins, until this is dropped. The
    /// pointer file is not changed, a pointer in a commit keeps its pin,
    /// and [`Repositories::lock`] refuses every write.
    pub fn read_at(&mut self, checkout: Checkout) {
        self.checkout = Some(checkout);
    }

    /// The mount pointer this run reads at a checkout, if any.
    pub fn checkout(&self) -> Option<&Checkout> {
        self.checkout.as_ref()
    }

    /// `mount`, a pointer at the physical `path` in `source`, with the
    /// commit this run reads it at.
    pub(crate) fn read_as(&self, source: &Source, path: &str, mut mount: Mount) -> Mount {
        if let (Source::WorkingTree, Some(checkout)) = (source, &self.checkout)
            && checkout.pointer == path
        {
            mount.commit = checkout.commit;
        }
        mount
    }

    /// The board's write lock, waiting up to `wait`. A run that reads a
    /// mount at a checkout writes nothing, since every write is checked
    /// against the commit the mount pins, so it is refused with
    /// `board::cli::usage`.
    pub fn lock(&self, wait: Duration) -> Result<Lock, WriteError> {
        self.writable()?;
        self.root.lock(wait)
    }

    /// Sets what the board's `min_board` makes of writes and syncs, and
    /// lifts the earlier setting when `gate` is `None`.
    pub fn gate_writes(&mut self, gate: Option<WriteGate>) {
        self.gate = gate;
    }

    /// Whether this run may write: it may not while a mount is read at a
    /// checkout, or when `min_board` refuses this Board.
    pub fn writable(&self) -> Result<(), WriteError> {
        match &self.gate {
            Some(WriteGate::TooOld {
                min_board,
                running,
                install,
            }) => {
                return Err(WriteError::BoardTooOld {
                    min_board: min_board.clone(),
                    running: running.clone(),
                    install: install.clone(),
                });
            }
            Some(WriteGate::Unreadable { detail }) => {
                return Err(WriteError::MinBoardUnreadable {
                    detail: detail.clone(),
                });
            }
            Some(WriteGate::Unchecked { note }) => UNCHECKED.with(|notes| {
                let mut notes = notes.borrow_mut();
                if !notes.contains(note) {
                    notes.push(note.clone());
                }
            }),
            None => {}
        }
        match &self.checkout {
            Some(checkout) => Err(WriteError::AtCheckout {
                pointer: checkout.pointer.clone(),
            }),
            None => Ok(()),
        }
    }

    /// What a mount's `~/` means.
    pub fn home(&self) -> Option<&Path> {
        self.home.as_deref()
    }

    /// How many repositories have been opened from disk since this was
    /// made, not counting the board's own. Work-count tests assert it.
    pub fn opened(&self) -> usize {
        self.opened.load(Ordering::Relaxed)
    }

    /// The repository `board` is in.
    pub fn repository(&self, board: &Board) -> Result<Repository, GitError> {
        self.lend(&board.repository)
    }

    /// The file at `location`, whose blob was `blob` when it was walked,
    /// read with `root`, the root repository's reader, when it is in the
    /// working tree, since only the root board is.
    pub fn read(
        &self,
        root: &mut Reader<'_>,
        location: &Location,
        blob: ObjectId,
    ) -> Result<Option<Contents>, GitError> {
        let (source, path) = (&location.board.source, &location.path);
        match source {
            Source::WorkingTree => root.read(source, path, blob),
            Source::Commit(_) => self
                .repository(&location.board)?
                .reader()
                .read(source, path, blob),
        }
    }

    /// The repository `mount`, a pointer in the repository at `from`, names:
    /// its `repo` read against `from`, or against the home directory for
    /// `~/`. `None` when it cannot be opened.
    pub(crate) fn mounted(&self, from: &Path, mount: &Mount) -> Option<Repository> {
        self.lend(&self.named(from, mount)?).ok()
    }

    /// The path of the repository `mount`, a pointer in the repository at
    /// `from`, names, or `None` for a `~/` path without a home.
    pub(crate) fn named(&self, from: &Path, mount: &Mount) -> Option<PathBuf> {
        Some(match mount.repo.strip_prefix("~/") {
            Some(rest) => self.home.as_deref()?.join(rest),
            None => from.join(&mount.repo),
        })
    }

    /// Opens each canonical path among `wanted` that is not yet open, in path
    /// order, one thread each, and looks up its commits there, so that
    /// [`Repositories::lend`] finds the repository open and its object
    /// database warm. Two paths that differ once canonicalised but open one
    /// repository, such as a working tree and its git directory, may both
    /// open it; the first to finish is kept and counted, and the other
    /// dropped. A path that cannot be opened is left for `lend` to refuse.
    pub(crate) fn open_all(&self, wanted: &[(PathBuf, ObjectId)]) {
        let mut closed: BTreeMap<PathBuf, Vec<ObjectId>> = BTreeMap::new();
        for (path, commit) in wanted {
            let path = path.canonicalize().unwrap_or_else(|_| path.clone());
            closed.entry(path).or_default().push(*commit);
        }
        closed.retain(|path, _| !self.open_map().contains_key(path));
        let (open, opened) = (&self.open, &self.opened);
        std::thread::scope(|scope| {
            for (path, commits) in closed {
                scope.spawn(move || {
                    let Ok(repository) = Repository::open(&path) else {
                        return;
                    };
                    for commit in commits {
                        let _ = repository.commit_tree(commit);
                    }
                    let mut open = open.lock().unwrap_or_else(PoisonError::into_inner);
                    if !open.contains_key(repository.path()) {
                        opened.fetch_add(1, Ordering::Relaxed);
                        Opened::of(&repository).insert(&mut open);
                    }
                });
            }
        });
    }

    /// The repository at `path`, opened from disk the first time any path
    /// that opens it, or a path that canonicalises to one, names it.
    fn lend(&self, path: &Path) -> Result<Repository, GitError> {
        let mut open = self.open_map();
        if let Some(found) = open.get(path) {
            return Ok(found.lend());
        }
        let canonical = path.canonicalize().ok();
        if let Some(found) = canonical.as_ref().and_then(|canonical| open.get(canonical)) {
            return Ok(found.lend());
        }
        let repository = Repository::open(path)?;
        self.opened.fetch_add(1, Ordering::Relaxed);
        Opened::of(&repository).insert(&mut open);
        Ok(repository)
    }

    /// The cache. A thread that panicked while holding it left it whole:
    /// an insert either happened or did not.
    fn open_map(&self) -> MutexGuard<'_, HashMap<PathBuf, Opened>> {
        self.open.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
