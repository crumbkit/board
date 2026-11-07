//! Whether a checkout's tracked files differ from the commit HEAD names.

use std::collections::HashMap;
use std::fs::{self, Metadata};
use std::path::Path;
use std::time::UNIX_EPOCH;

use gix::bstr::BStr;
use gix::index::entry::{Mode, Stage};

use crate::git::model_id;
use crate::worktree::settle::hash_stream;

/// Whether the working tree holding `directory` has an uncommitted change
/// to a file git tracks, as `git status --untracked-files=no` would list
/// one: a file of HEAD's tree that is gone or holds other bytes, or an
/// index entry that differs from HEAD's tree. Untracked files and
/// submodules count for nothing. `None` when `directory` is in no
/// repository, HEAD names no commit, or git's files cannot be read.
///
/// A file whose size and modification time match its index entry, and
/// which was modified before the index was written, is taken as unchanged
/// without reading it, as git does. Any other file is hashed. A file is
/// hashed as it is on disk, without git's filters, so a checkout that
/// converts line endings can count as changed.
pub fn uncommitted(directory: &Path) -> Option<bool> {
    let git = gix::discover(directory).ok()?;
    let workdir = git.workdir()?.to_path_buf();
    let committed = git.index_from_tree(&git.head_tree_id().ok()?).ok()?;
    let index = git.index_or_empty().ok()?;
    let written = fs::metadata(git.index_path())
        .ok()
        .map(|file| seconds(&file));
    let staged: HashMap<&BStr, &gix::index::Entry> = (index.entries().iter())
        .map(|entry| (entry.path(&index), entry))
        .collect();
    if (index.entries().iter()).any(|entry| entry.stage() != Stage::Unconflicted) {
        return Some(true);
    }
    let mut matched = 0;
    for entry in committed.entries() {
        let path = entry.path(&committed);
        let staged = staged.get(path).copied();
        if let Some(staged) = staged {
            matched += 1;
            if staged.id != entry.id || staged.mode != entry.mode {
                return Some(true);
            }
        }
        if entry.mode == Mode::COMMIT {
            continue;
        }
        let file = workdir.join(gix::path::from_bstr(path));
        let Ok(metadata) = fs::symlink_metadata(&file) else {
            return Some(true);
        };
        let unchanged = staged.is_some_and(|staged| same_stat(staged, &metadata, written));
        if !unchanged && !holds(&git, entry, &file, &metadata) {
            return Some(true);
        }
    }
    // An index entry HEAD's tree does not hold is a staged new file.
    Some(matched != index.entries().len())
}

/// Whether `file` holds the blob `entry` names, as a symlink when the entry
/// is one and as a regular file otherwise.
fn holds(
    git: &gix::Repository,
    entry: &gix::index::Entry,
    file: &Path,
    metadata: &Metadata,
) -> bool {
    let kind = git.object_hash();
    let id = if entry.mode == Mode::SYMLINK {
        if !metadata.is_symlink() {
            return false;
        }
        let Ok(target) = fs::read_link(file) else {
            return false;
        };
        let bytes = gix::path::into_bstr(target).into_owned();
        let length = u64::try_from(bytes.len()).expect("a link's target fits in u64");
        hash_stream(kind, &mut bytes.as_slice(), length)
    } else {
        if !metadata.is_file() {
            return false;
        }
        let Ok(mut opened) = fs::File::open(file) else {
            return false;
        };
        hash_stream(kind, &mut opened, metadata.len())
    };
    id == Some(model_id(&entry.id))
}

/// Whether `metadata` has the size and modification time `staged` records,
/// for a file modified before the index was `written`; a file modified in
/// the same second or later may have changed since.
fn same_stat(staged: &gix::index::Entry, metadata: &Metadata, written: Option<u64>) -> bool {
    let Ok(modified) = metadata.modified() else {
        return false;
    };
    let Ok(since) = modified.duration_since(UNIX_EPOCH) else {
        return false;
    };
    u64::from(staged.stat.size) == metadata.len()
        && u64::from(staged.stat.mtime.secs) == since.as_secs()
        && staged.stat.mtime.nsecs == since.subsec_nanos()
        && written.is_some_and(|written| since.as_secs() < written)
}

/// The second `metadata` was last modified, or 0.
fn seconds(metadata: &Metadata) -> u64 {
    (metadata.modified().ok())
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |since| since.as_secs())
}
