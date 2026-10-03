//! The one module that reads the working tree: every file git sees, which is
//! tracked files and untracked files that are not ignored.

use std::error::Error;
use std::fs::{self, FileType};
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use board_model::{MAX_RECORD_BYTES, ObjectId, Source};
use gix::bstr::{BString, ByteSlice};
use gix::index::entry::Mode;
use gix::worktree::stack::state::ignore;

use crate::git::{GitError, Repository, model_id};
use crate::tree::{Kind, RawEntry, Tree};

/// The working tree of a repository, with its index and ignore rules.
pub(crate) struct WorkingTree<'r> {
    repository: &'r Repository,
    index: gix::worktree::Index,
    excludes: gix::AttributeStack<'r>,
}

impl Repository {
    pub(crate) fn working_tree(&self) -> Result<WorkingTree<'_>, GitError> {
        let unreadable = |source: Box<dyn Error + Send + Sync>| GitError::Unreadable {
            repository: self.path().to_path_buf(),
            source,
        };
        if self.git().workdir().is_none() {
            return Err(GitError::NoWorkingTree {
                repository: self.path().to_path_buf(),
            });
        }
        let index = self
            .git()
            .index_or_empty()
            .map_err(|error| unreadable(error.into()))?;
        let excludes = self
            .git()
            .excludes(
                &index,
                None,
                ignore::Source::WorktreeThenIdMappingIfNotSkipped,
            )
            .map_err(|error| unreadable(error.into()))?;
        Ok(WorkingTree {
            repository: self,
            index,
            excludes,
        })
    }
}

impl WorkingTree<'_> {
    /// What git sees at `relative`, or `None` when git does not see it: an
    /// untracked path that is ignored, or something other than a file,
    /// directory or symlink. A path whose ignore rules or contents cannot be
    /// read is `Unreadable`.
    fn kind(&mut self, relative: &Path, file_type: FileType) -> Option<Kind<PathBuf>> {
        let key = gix::path::to_unix_separators_on_windows(gix::path::into_bstr(relative));
        let indexed = self.index.entry_by_path(key.as_ref());
        let tracked = indexed.is_some()
            || file_type.is_dir() && {
                let mut prefix = BString::from(key.as_bytes());
                prefix.push(b'/');
                self.index.prefixed_entries(prefix.as_ref()).is_some()
            };
        if !tracked {
            let mode = file_type.is_dir().then_some(Mode::DIR);
            match self.excludes.at_path(relative, mode) {
                Ok(platform) if platform.is_excluded() => return None,
                Ok(_) => {}
                Err(_) => return Some(Kind::Unreadable),
            }
        }
        let absolute = self.repository.path().join(relative);
        let kind = if file_type.is_dir() {
            match indexed {
                Some(entry) if entry.mode == Mode::COMMIT => Kind::Submodule,
                _ if absolute.join(".git").exists() => Kind::Submodule,
                _ => Kind::Directory(relative.to_path_buf()),
            }
        } else if file_type.is_symlink() {
            fs::read_link(&absolute)
                .ok()
                .and_then(|target| {
                    let target = gix::path::into_bstr(target);
                    self.hash(&mut target.as_bytes(), target.len() as u64)
                })
                .map_or(Kind::Unreadable, Kind::Symlink)
        } else if file_type.is_file() {
            fs::File::open(&absolute)
                .and_then(|file| Ok((file.metadata()?.len(), file)))
                .ok()
                .and_then(|(length, file)| self.hash(&mut BufReader::new(file), length))
                .map_or(Kind::Unreadable, Kind::File)
        } else {
            return None;
        };
        Some(kind)
    }

    /// The id git gives a blob of `length` bytes read from `stream`, in
    /// constant memory; `None` if the stream fails, or yields fewer or more
    /// bytes than `length` because the file changed while it was read.
    fn hash(&self, stream: &mut dyn Read, length: u64) -> Option<ObjectId> {
        let id = gix::objs::compute_stream_hash(
            self.repository.git().object_hash(),
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
}

impl Tree for WorkingTree<'_> {
    /// A directory's path relative to the working directory.
    type Directory = PathBuf;

    fn source(&self) -> Source {
        Source::WorkingTree
    }

    fn root(&self) -> PathBuf {
        PathBuf::new()
    }

    fn list(&mut self, directory: &PathBuf) -> Result<Vec<RawEntry<PathBuf>>, GitError> {
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
                Err(_) => Some(Kind::Unreadable),
            };
            if let Some(kind) = kind {
                entries.push(RawEntry {
                    name: gix::path::into_bstr(Path::new(&name)).to_vec(),
                    kind,
                });
            }
        }
        Ok(entries)
    }

    fn read(
        &mut self,
        directory: &PathBuf,
        entry: &RawEntry<PathBuf>,
    ) -> Result<Option<Vec<u8>>, GitError> {
        read_file(
            &self
                .repository
                .path()
                .join(directory)
                .join(gix::path::from_bstr(entry.name.as_bstr())),
        )
    }
}

/// The bytes of the file at `path`, or `None` when it holds more than
/// [`MAX_RECORD_BYTES`].
pub(crate) fn read_file(path: &Path) -> Result<Option<Vec<u8>>, GitError> {
    let unreadable = |source| GitError::File {
        path: path.to_path_buf(),
        source,
    };
    let file = fs::File::open(path).map_err(unreadable)?;
    let mut bytes = Vec::new();
    file.take(MAX_RECORD_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(unreadable)?;
    Ok((bytes.len() <= MAX_RECORD_BYTES).then_some(bytes))
}

/// Whether `directory` is the root of a repository's working tree: it holds
/// `.git`, as a directory or as the file a linked worktree has.
pub fn is_repository_root(directory: &Path) -> bool {
    directory.join(".git").exists()
}

/// The directory holding `.board/board.json` nearest above `start`, or
/// `start` itself, searching upward as git does.
pub fn discover(start: &Path) -> Option<PathBuf> {
    let start = start.canonicalize().ok()?;
    start
        .ancestors()
        .find(|directory| directory.join(".board").join("board.json").is_file())
        .map(Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stream_longer_or_shorter_than_its_length_has_no_id() {
        let directory = tempfile::tempdir().unwrap();
        gix::init(directory.path()).unwrap();
        let repository = Repository::open(directory.path()).unwrap();
        let tree = repository.working_tree().unwrap();
        let expected =
            gix::objs::compute_hash(gix::hash::Kind::Sha1, gix::objs::Kind::Blob, b"abc").unwrap();
        assert_eq!(tree.hash(&mut &b"abc"[..], 3), Some(model_id(&expected)));
        assert_eq!(tree.hash(&mut &b"abcd"[..], 3), None);
        assert_eq!(tree.hash(&mut &b"ab"[..], 3), None);
    }
}
