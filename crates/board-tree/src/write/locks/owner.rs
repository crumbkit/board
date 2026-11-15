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
use std::time::UNIX_EPOCH;

/// A file's birth time, seconds and nanoseconds since the epoch.
type Birth = (u64, u32);

/// What tells one lock file from another that was created at its path
/// later: the device and inode, and the birth time, which a file system
/// that reuses an inode number gives the new file anew. `birth` is `None`
/// where the platform or file system gives none, and a file with no birth
/// time is never taken for the one an entry names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Identity {
    device: u64,
    inode: u64,
    birth: Option<Birth>,
}

impl Identity {
    fn of(metadata: &fs::Metadata) -> Self {
        let birth = metadata
            .created()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|since| (since.as_secs(), since.subsec_nanos()));
        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            birth,
        }
    }

    /// Whether the file with identity `found` is the file this identity
    /// was taken from.
    fn names(&self, found: &Self) -> bool {
        self.birth.is_some() && self == found
    }
}

/// One line of the record: the identity of the lock file at `path` when
/// Board took it.
#[derive(Debug, PartialEq, Eq)]
struct Entry {
    identity: Identity,
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
                identity: Identity::of(&metadata),
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

impl Owned {
    /// Removes the entries now, for lock files that were committed or
    /// renamed away. The record that cannot be rewritten stays, naming
    /// files that the next run finds gone or finds with another inode.
    pub(in crate::write) fn release(&mut self) {
        if !self.held.is_empty() {
            let mut entries = read(&self.record);
            entries.retain(|entry| !self.held.contains(&entry.path));
            let _ = write(&self.record, &entries);
            self.held.clear();
        }
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        self.release();
    }
}

/// Removes each lock file `record` names whose device, inode and birth time
/// are the ones its entry holds, which only a killed Board process leaves,
/// and then the record. A file with the entry's path and another identity,
/// or no birth time, or no file, was not left by a killed process, so it
/// stays. The caller must hold `board.flock`, so no other Board process
/// changes the record or holds a lock meanwhile. A lock file that cannot be
/// removed is returned with the error.
pub(in crate::write) fn clear(record: &Path) -> Result<(), (PathBuf, io::Error)> {
    clear_with(record, Identity::of)
}

/// [`clear`], with `identify` giving the identity of each file found.
fn clear_with(
    record: &Path,
    identify: impl Fn(&fs::Metadata) -> Identity,
) -> Result<(), (PathBuf, io::Error)> {
    for entry in read(record) {
        if let Ok(metadata) = fs::symlink_metadata(&entry.path)
            && entry.identity.names(&identify(&metadata))
        {
            match fs::remove_file(&entry.path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err((entry.path, error)),
            }
        }
    }
    write(record, &[]).map_err(|error| (record.to_path_buf(), error))
}

/// The entries of `record`; none when it is missing or unreadable, and a
/// line that does not parse is left out. A line of the earlier format,
/// which holds no birth time, reads with none.
fn read(record: &Path) -> Vec<Entry> {
    let Ok(text) = fs::read_to_string(record) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| {
            let (device, rest) = line.split_once(' ')?;
            let (inode, rest) = rest.split_once(' ')?;
            let (birth, path) = if rest.starts_with('/') {
                (None, rest)
            } else {
                let (birth, path) = rest.split_once(' ')?;
                (parse_birth(birth), path)
            };
            Some(Entry {
                identity: Identity {
                    device: device.parse().ok()?,
                    inode: inode.parse().ok()?,
                    birth,
                },
                path: PathBuf::from(path),
            })
        })
        .collect()
}

/// `seconds.nanoseconds`, or none for `-` and for text that is not one.
fn parse_birth(text: &str) -> Option<Birth> {
    let (seconds, nanoseconds) = text.split_once('.')?;
    Some((seconds.parse().ok()?, nanoseconds.parse().ok()?))
}

/// Replaces `record` with `entries` by temporary file and rename, or
/// removes it when there are none. The record is not fsynced: a killed
/// process leaves the page cache, which the next run reads, and the fsync
/// would lengthen the window between taking a lock and recording it.
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
            let Identity {
                device,
                inode,
                birth,
            } = entry.identity;
            let birth = birth.map_or_else(|| "-".to_owned(), |(s, n)| format!("{s}.{n:09}"));
            Some(format!("{device} {inode} {birth} {path}\n"))
        })
        .collect();
    let temporary = record.with_extension("tmp");
    fs::write(&temporary, text).and_then(|()| fs::rename(&temporary, record))
}

#[cfg(test)]
mod tests;
