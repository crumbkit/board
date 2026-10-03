//! The one module that reads the working tree: every file git sees, which is
//! tracked files and untracked files that are not ignored.

use std::error::Error;
use std::fs::{self, FileType};
use std::path::{Path, PathBuf};

use board_model::{ObjectId, Source};
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
    /// directory or symlink.
    fn kind(
        &mut self,
        relative: &Path,
        file_type: FileType,
    ) -> Result<Option<Kind<PathBuf>>, GitError> {
        let key = gix::path::to_unix_separators_on_windows(gix::path::into_bstr(relative));
        let indexed = self.index.entry_by_path(key.as_ref());
        let tracked = indexed.is_some()
            || file_type.is_dir() && {
                let mut prefix = BString::from(key.as_bytes());
                prefix.push(b'/');
                self.index.prefixed_entries(prefix.as_ref()).is_some()
            };
        let absolute = self.repository.path().join(relative);
        let unreadable = |source| GitError::File {
            path: absolute.clone(),
            source,
        };
        if !tracked {
            let mode = file_type.is_dir().then_some(Mode::DIR);
            let excluded = self
                .excludes
                .at_path(relative, mode)
                .map_err(unreadable)?
                .is_excluded();
            if excluded {
                return Ok(None);
            }
        }
        Ok(Some(if file_type.is_dir() {
            match indexed {
                Some(entry) if entry.mode == Mode::COMMIT => {
                    Kind::Submodule(Some(model_id(&entry.id)))
                }
                _ if absolute.join(".git").exists() => Kind::Submodule(None),
                _ => Kind::Directory(relative.to_path_buf()),
            }
        } else if file_type.is_symlink() {
            let target = fs::read_link(&absolute).map_err(unreadable)?;
            Kind::Symlink(self.blob(&gix::path::into_bstr(target))?)
        } else if file_type.is_file() {
            Kind::File(self.blob(&fs::read(&absolute).map_err(unreadable)?)?)
        } else {
            return Ok(None);
        }))
    }

    /// The id git gives a blob holding `bytes`.
    fn blob(&self, bytes: &[u8]) -> Result<ObjectId, GitError> {
        gix::objs::compute_hash(
            self.repository.git().object_hash(),
            gix::objs::Kind::Blob,
            bytes,
        )
        .map(|id| model_id(&id))
        .map_err(|error| GitError::Unreadable {
            repository: self.repository.path().to_path_buf(),
            source: error.into(),
        })
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
            let file_type = item.file_type().map_err(unreadable)?;
            if let Some(kind) = self.kind(&directory.join(&name), file_type)? {
                entries.push(RawEntry {
                    name: gix::path::into_bstr(Path::new(&name)).to_vec(),
                    kind,
                });
            }
        }
        Ok(entries)
    }
}
