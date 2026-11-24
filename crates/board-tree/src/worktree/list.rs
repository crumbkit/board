//! Listing the directories of a walk on several threads, ahead of the
//! thread that walks them.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::panic;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Mutex, PoisonError};
use std::thread;

use board_model::Stat;
use gix::bstr::ByteSlice;

use crate::git::GitError;
use crate::tree::Visit;
use crate::worktree::{WorkingTree, stat};

/// One item of a directory as the file system lists it: its name as git
/// spells it, and what it is.
pub(crate) struct Raw {
    pub(crate) name: Vec<u8>,
    pub(crate) kind: RawKind,
}

pub(crate) enum RawKind {
    /// A directory, and whether it holds `.git`.
    Directory { has_git: bool },
    /// A file, with its metadata.
    File(Stat),
    /// A symlink, with its metadata and its target, if that can be read.
    Symlink { stat: Stat, target: Option<Vec<u8>> },
    /// An item whose type or metadata cannot be read.
    Unreadable,
}

/// One item of a directory as `read_dir` gives it, before any `lstat`: its
/// name as git spells it, and its type, `None` when that cannot be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub name: Vec<u8>,
    pub file_type: Option<ItemType>,
}

/// What a directory lists an item as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemType {
    Directory,
    Symlink,
    File,
}

impl ItemType {
    fn of(file_type: fs::FileType) -> Option<Self> {
        if file_type.is_dir() {
            Some(Self::Directory)
        } else if file_type.is_symlink() {
            Some(Self::Symlink)
        } else if file_type.is_file() {
            Some(Self::File)
        } else {
            None
        }
    }
}

/// One directory's items as a walk listed them, with the directory's
/// metadata just before, and when that walk began.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectoryListing {
    pub stat: Stat,
    pub listed_ns: i64,
    pub items: Vec<Item>,
}

/// What a walk listed in each directory, by its path in the working tree.
pub type Directories = HashMap<PathBuf, DirectoryListing>;

impl DirectoryListing {
    /// Whether these items are still the directory's, now that its
    /// metadata is `seen` and the walk began at `now_ns`: the metadata is
    /// unchanged, this listing [lasts](DirectoryListing::lasts), and the
    /// walk that listed it began no later than this one.
    fn holds(&self, seen: &Stat, now_ns: i64) -> bool {
        self.stat == *seen && self.lasts() && self.listed_ns <= now_ns
    }

    /// Whether a later walk may trust this listing: as in git's racy-clean
    /// rule, the directory was modified in an earlier whole second than the
    /// walk that listed it began, so no change in that second can be
    /// missing from it.
    pub fn lasts(&self) -> bool {
        const SECOND: i64 = 1_000_000_000;
        self.stat.modified_ns.div_euclid(SECOND) < self.listed_ns.div_euclid(SECOND)
    }
}

/// The items of `directory` beneath `root`, sorted by name, leaving out
/// `.git` at the root and anything that is not a file, directory or
/// symlink, and how they were listed. When `earlier` holds the directory
/// and still [holds](DirectoryListing::holds) for the walk begun at `now_ns`, its
/// items are read from there; otherwise the directory is listed, and the
/// listing counted in `listings`. `DirEntry::file_type` takes each type
/// from the listing, and calls `lstat` only where the file system does not
/// give it (`DT_UNKNOWN`). The directory's own metadata is read first, so a
/// change while it is listed shows as a change the next time.
pub(super) fn items(
    (root, directory): (&Path, &Path),
    listings: &AtomicUsize,
    (earlier, now_ns): (&Directories, i64),
) -> io::Result<(Vec<Item>, Option<DirectoryListing>)> {
    let absolute = root.join(directory);
    let seen = fs::symlink_metadata(&absolute)
        .ok()
        .map(|found| stat(&found));
    if let (Some(seen), Some(listed)) = (&seen, earlier.get(directory))
        && listed.holds(seen, now_ns)
    {
        return Ok((listed.items.clone(), Some(listed.clone())));
    }
    listings.fetch_add(1, Ordering::Relaxed);
    let mut items = Vec::new();
    for item in fs::read_dir(&absolute)? {
        let item = item?;
        let name = item.file_name();
        let file_type = item.file_type().ok().map(ItemType::of);
        if directory.as_os_str().is_empty() && name == ".git" || file_type == Some(None) {
            continue;
        }
        items.push(Item {
            name: gix::path::into_bstr(Path::new(&name)).to_vec(),
            file_type: file_type.flatten(),
        });
    }
    items.sort_by(|a, b| a.name.cmp(&b.name));
    let listed = seen.map(|stat| DirectoryListing {
        stat,
        listed_ns: now_ns,
        items: items.clone(),
    });
    Ok((items, listed))
}

/// `item` of the directory at `absolute`, with what it is and its metadata.
pub(super) fn raw(absolute: &Path, item: Item) -> Raw {
    let path = &absolute.join(gix::path::from_bstr(item.name.as_bstr()));
    let kind = match item.file_type {
        None => RawKind::Unreadable,
        Some(ItemType::Directory) => RawKind::Directory {
            has_git: path.join(".git").exists(),
        },
        Some(ItemType::Symlink) => match fs::symlink_metadata(path) {
            Ok(metadata) => RawKind::Symlink {
                stat: stat(&metadata),
                target: fs::read_link(path)
                    .ok()
                    .map(|target| gix::path::into_bstr(target).to_vec()),
            },
            Err(_) => RawKind::Unreadable,
        },
        Some(_) => match fs::metadata(path) {
            Ok(metadata) => RawKind::File(stat(&metadata)),
            Err(_) => RawKind::Unreadable,
        },
    };
    Raw {
        name: item.name,
        kind,
    }
}

/// Lists `directory` beneath `root` as [`items`] does, with each item's
/// metadata, and how it was listed.
pub(super) fn read(
    (root, directory): (&Path, &Path),
    listings: &AtomicUsize,
    earlier: (&Directories, i64),
) -> io::Result<(Vec<Raw>, Option<DirectoryListing>)> {
    let absolute = root.join(directory);
    let (items, listed) = items((root, directory), listings, earlier)?;
    let raw = items.into_iter().map(|item| raw(&absolute, item)).collect();
    Ok((raw, listed))
}

/// A directory to list, with the payload the walk gave it, which comes
/// back with the listing.
type ToList<P> = (PathBuf, P);

/// A directory listed, with its payload: the listing and how it was listed,
/// or the payload of the panic that listing it raised.
type Reply<P> = (
    PathBuf,
    P,
    thread::Result<io::Result<(Vec<Raw>, Option<DirectoryListing>)>>,
);

/// Lists the directories received from `take`, one at a time, until the
/// walk stops receiving, or until it has dropped its sender and every
/// directory already sent has been listed. Only one thread waits on the
/// receiver; the others wait for the lock. Every directory received gets
/// one reply: a panic while listing it is caught and sent as the reply.
fn list_from<P>(
    (root, listings, earlier): (&Path, &AtomicUsize, (&Directories, i64)),
    take: &Mutex<Receiver<ToList<P>>>,
    send: &Sender<Reply<P>>,
) {
    loop {
        let received = take.lock().unwrap_or_else(PoisonError::into_inner).recv();
        let Ok((directory, payload)) = received else {
            return;
        };
        let listed = panic::catch_unwind(|| read((root, &directory), listings, earlier));
        if send.send((directory, payload, listed)).is_err() {
            return;
        }
    }
}

impl<T: Send> WorkingTree<'_, '_, T> {
    /// Visits as [`crate::tree::Tree::visit_all`] does, listing directories
    /// on the repository's threads ahead of `visit`, which runs on this
    /// thread: a directory is listed as soon as `visit` returns it.
    ///
    /// # Panics
    ///
    /// When a listing thread panics, with its payload, as if the panic
    /// were here: the thread catches it and sends it back as the
    /// directory's listing, and this thread raises it. `read` makes no
    /// call that panics today.
    pub(crate) fn visit_ahead<P: Send>(
        &mut self,
        start: (PathBuf, P),
        visit: Visit<'_, Self, P>,
    ) -> Result<(), GitError> {
        let repository = self.repository;
        let root = repository.path();
        let listings = repository.listings();
        let earlier = (self.earlier, self.started_ns);
        let (to_list, take) = mpsc::channel();
        let (send, listed) = mpsc::channel();
        let take = &Mutex::new(take);
        thread::scope(|scope| {
            // The closure owns the sender, so returning or a panic in
            // `visit` drops it. Dropping it ends each listing thread once
            // the directories already sent have been listed, and the scope
            // then joins them. A listing thread catches its own panics, so
            // the join raises none.
            let to_list = to_list;
            for _ in 0..repository.threads() {
                let send = send.clone();
                scope.spawn(move || list_from((root, listings, earlier), take, &send));
            }
            drop(send);
            self.visit_received(start, visit, &to_list, &listed)
        })
    }

    /// Visits each directory as its listing arrives from `listed`, sending
    /// the directories `visit` returns to `to_list`, until every directory
    /// sent has been visited. A listing that is a panic's payload is raised
    /// here.
    pub(crate) fn visit_received<P>(
        &mut self,
        start: (PathBuf, P),
        visit: Visit<'_, Self, P>,
        to_list: &Sender<ToList<P>>,
        listed: &Receiver<Reply<P>>,
    ) -> Result<(), GitError> {
        let root = self.repository.path();
        let mut awaited: usize = 0;
        let mut next = vec![start];
        loop {
            for directory in next {
                to_list
                    .send(directory)
                    .expect("the walk holds the receiver until every listing thread has ended");
                awaited += 1;
            }
            if awaited == 0 {
                return Ok(());
            }
            // A listing thread ends only once this sender is dropped or
            // the receiver is, and both outlive this loop; a panic while
            // listing is caught on the thread, so it does not end it.
            let (directory, payload, raw) = listed
                .recv()
                .expect("a listing thread lives until the walk drops its sender");
            awaited -= 1;
            let raw = raw.unwrap_or_else(|payload| panic::resume_unwind(payload));
            let listed = raw
                .map(|(raw, listed)| {
                    if let Some(listed) = listed {
                        self.directories.insert(directory.clone(), listed);
                    }
                    self.classify(&directory, raw)
                })
                .map_err(|source| GitError::File {
                    path: root.join(&directory),
                    source,
                });
            next = visit(self, (directory, payload), listed)?;
        }
    }
}
