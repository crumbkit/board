//! The lock owner record, `board-locks` in the git directory: for each git
//! lock file Board holds, the file's device and inode. A SIGKILL leaves the
//! lock file and its entry behind, so the next put or sync, which holds
//! `board.flock`, finds the file the entry names and removes it. A lock
//! file the record does not name, such as the one a `git commit` holds,
//! is never removed (docs/architecture/writes/commit.md).

use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use super::super::write_temporary;

/// One line of the record: the device and inode of the lock file at `path`
/// when Board took it.
#[derive(Debug, PartialEq, Eq)]
struct Entry {
    device: u64,
    inode: u64,
    path: PathBuf,
}

/// The entries for the lock files Board holds. Dropping it removes them from
/// the record, which Board does once the locks are released or committed.
#[derive(Debug)]
pub(in crate::write) struct Owned {
    record: PathBuf,
    held: Vec<PathBuf>,
}

impl Owned {
    /// An owner that records nothing.
    pub(in crate::write) const fn none() -> Self {
        Self {
            record: PathBuf::new(),
            held: Vec::new(),
        }
    }

    /// Records in `record` that this process holds the lock files at
    /// `locks`, those of them that exist. A record that cannot be written
    /// leaves the locks unrecorded: a kill then leaves lock files that a
    /// person removes.
    pub(in crate::write) fn claim(record: &Path, locks: &[&Path]) -> Self {
        let mut entries = read(record);
        let mut held = Vec::new();
        for lock in locks {
            let Ok(metadata) = fs::symlink_metadata(lock) else {
                continue;
            };
            if lock.to_str().is_none_or(|text| text.contains('\n')) {
                continue;
            }
            entries.retain(|entry| entry.path != *lock);
            entries.push(Entry {
                device: metadata.dev(),
                inode: metadata.ino(),
                path: lock.to_path_buf(),
            });
            held.push(lock.to_path_buf());
        }
        match write(record, &entries) {
            Ok(()) => Self {
                record: record.to_path_buf(),
                held,
            },
            Err(_) => Self::none(),
        }
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        if !self.held.is_empty() {
            let mut entries = read(&self.record);
            entries.retain(|entry| !self.held.contains(&entry.path));
            // A record that cannot be rewritten stays, naming files that
            // the next run finds gone or finds with another inode.
            let _ = write(&self.record, &entries);
        }
    }
}

/// Removes each lock file `record` names whose device and inode are the ones
/// its entry holds, which only a killed Board process leaves, and then the
/// record. A file with the entry's path and another inode, or no file, was
/// not left by a killed process, so it stays. The caller must hold
/// `board.flock`, so no other Board process changes the record or holds a
/// lock meanwhile.
pub(in crate::write) fn clear(record: &Path) -> io::Result<()> {
    for entry in read(record) {
        if let Ok(metadata) = fs::symlink_metadata(&entry.path)
            && (metadata.dev(), metadata.ino()) == (entry.device, entry.inode)
        {
            match fs::remove_file(&entry.path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
    }
    write(record, &[])
}

/// The entries of `record`; none when it is missing or unreadable, and a
/// line that does not parse is left out.
fn read(record: &Path) -> Vec<Entry> {
    let Ok(text) = fs::read_to_string(record) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| {
            let (device, rest) = line.split_once(' ')?;
            let (inode, path) = rest.split_once(' ')?;
            Some(Entry {
                device: device.parse().ok()?,
                inode: inode.parse().ok()?,
                path: PathBuf::from(path),
            })
        })
        .collect()
}

/// Replaces `record` with `entries` by temporary file and rename, or
/// removes it when there are none.
fn write(record: &Path, entries: &[Entry]) -> io::Result<()> {
    if entries.is_empty() {
        return match fs::remove_file(record) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        };
    }
    let text: String = entries
        .iter()
        .filter_map(|entry| {
            let path = entry.path.to_str()?;
            Some(format!("{} {} {path}\n", entry.device, entry.inode))
        })
        .collect();
    let temporary = record.with_extension("tmp");
    write_temporary(&temporary, record, text.as_bytes())
        .and_then(|()| fs::rename(&temporary, record))
}

#[cfg(test)]
mod tests;
