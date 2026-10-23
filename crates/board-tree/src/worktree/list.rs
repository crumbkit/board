//! Listing the directories of a walk on several threads, ahead of the
//! thread that walks them.

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
#[derive(Clone)]
pub(crate) struct Item {
    pub(crate) name: Vec<u8>,
    pub(super) file_type: Option<fs::FileType>,
}

/// The items of `directory` beneath `root`, sorted by name, leaving out
/// `.git` at the root and anything that is not a file, directory or
/// symlink, and counts the listing in `listings`. `DirEntry::file_type`
/// takes each type from the listing, and calls `lstat` only where the file
/// system does not give it (`DT_UNKNOWN`).
pub(super) fn items(
    root: &Path,
    listings: &AtomicUsize,
    directory: &Path,
) -> io::Result<Vec<Item>> {
    listings.fetch_add(1, Ordering::Relaxed);
    let mut items = Vec::new();
    for item in fs::read_dir(root.join(directory))? {
        let item = item?;
        let name = item.file_name();
        let file_type = item.file_type().ok();
        let listed = |kind: fs::FileType| kind.is_dir() || kind.is_symlink() || kind.is_file();
        if directory.as_os_str().is_empty() && name == ".git" || !file_type.is_none_or(listed) {
            continue;
        }
        items.push(Item {
            name: gix::path::into_bstr(Path::new(&name)).to_vec(),
            file_type,
        });
    }
    items.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(items)
}

/// `item` of the directory at `absolute`, with what it is and its metadata.
pub(super) fn raw(absolute: &Path, item: Item) -> Raw {
    let path = &absolute.join(gix::path::from_bstr(item.name.as_bstr()));
    let kind = match item.file_type {
        None => RawKind::Unreadable,
        Some(file_type) if file_type.is_dir() => RawKind::Directory {
            has_git: path.join(".git").exists(),
        },
        Some(file_type) if file_type.is_symlink() => match fs::symlink_metadata(path) {
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
/// metadata.
pub(super) fn read(root: &Path, listings: &AtomicUsize, directory: &Path) -> io::Result<Vec<Raw>> {
    let absolute = root.join(directory);
    let items = items(root, listings, directory)?;
    Ok(items.into_iter().map(|item| raw(&absolute, item)).collect())
}

/// A directory to list, with the payload the walk gave it, which comes
/// back with the listing.
type ToList<P> = (PathBuf, P);

/// A directory listed, with its payload: the listing, or the payload of the
/// panic that listing it raised.
type Reply<P> = (PathBuf, P, thread::Result<io::Result<Vec<Raw>>>);

/// Lists the directories received from `take`, one at a time, until the
/// walk stops receiving, or until it has dropped its sender and every
/// directory already sent has been listed. Only one thread waits on the
/// receiver; the others wait for the lock. Every directory received gets
/// one reply: a panic while listing it is caught and sent as the reply.
fn list_from<P>(
    root: &Path,
    listings: &AtomicUsize,
    take: &Mutex<Receiver<ToList<P>>>,
    send: &Sender<Reply<P>>,
) {
    loop {
        let received = take.lock().unwrap_or_else(PoisonError::into_inner).recv();
        let Ok((directory, payload)) = received else {
            return;
        };
        let listed = panic::catch_unwind(|| read(root, listings, &directory));
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
                scope.spawn(move || list_from(root, listings, take, &send));
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
                .map(|raw| self.classify(&directory, raw))
                .map_err(|source| GitError::File {
                    path: root.join(&directory),
                    source,
                });
            next = visit(self, (directory, payload), listed)?;
        }
    }
}
