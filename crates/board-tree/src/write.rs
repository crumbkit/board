//! The one module that writes the working tree and the git directory
//! (docs/architecture.md, Writes): where a put lands, the lock, and the
//! write that a kill cannot leave half done.
#![expect(
    clippy::disallowed_methods,
    reason = "board-tree's write module owns every write to the working tree and the git directory"
)]

use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use board_model::{Code, Location, LogicalPath, MAX_RECORD_BYTES, Object, ObjectId, Source};

use crate::address;
use crate::git::{GitError, Repository, model_id};
use crate::resolve::ResolveError;
use crate::tree::{Kind, RawEntry, Tree};

/// How long a put waits for another put's lock.
pub const LOCK_WAIT: Duration = Duration::from_secs(5);
/// How often a waiting put tries the lock again.
const LOCK_POLL: Duration = Duration::from_millis(20);

/// Where a put writes: the record file a logical path names, existing or
/// not, in the working tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    /// The file's path in the working tree, `/`-separated.
    pub path: String,
    /// The Location resolution reached, with every pointer followed; for a
    /// new file, its parent directory's.
    pub location: Location,
}

/// The lock a put holds from comparing its `--expect` to renaming its file.
/// Dropping it removes the lock file.
#[derive(Debug)]
pub struct Lock {
    path: PathBuf,
}

impl Drop for Lock {
    fn drop(&mut self) {
        // A lock that cannot be removed is reported by the next put.
        let _ = fs::remove_file(&self.path);
    }
}

impl Repository {
    /// The record file `path` names for writing. Pointers are followed,
    /// including a link at the last segment; a path that reaches a mount is
    /// refused. A path that names nothing yet names a new file beneath the
    /// deepest directory that exists. `is_pointer` says whether the record
    /// to be written is a pointer, which would shadow a directory beside it;
    /// a write that would leave its own path degraded is refused with that
    /// path's code.
    pub fn target(&self, path: &LogicalPath, is_pointer: bool) -> Result<Target, WriteError> {
        let refused = |code| WriteError::Refused {
            path: path.clone(),
            code,
        };
        let (location, physical) = match self.resolve(&Source::WorkingTree, path) {
            Ok(location) => {
                let physical = record_file(&location).ok_or(refused(Code::RecordNotARecord))?;
                (location, physical)
            }
            Err(ResolveError::NotFound { .. }) => self.new_file(path)?,
            Err(error) => return Err(refused(error.code())),
        };
        if location.board.source != Source::WorkingTree {
            return Err(refused(Code::PutThroughMount));
        }
        if let Some(code) = self.degraded(&physical, is_pointer)? {
            return Err(refused(code));
        }
        Ok(Target {
            path: physical,
            location,
        })
    }

    /// The new file `path` names: beneath its deepest ancestor that
    /// resolves, with the segments past it as directories to create.
    fn new_file(&self, path: &LogicalPath) -> Result<(Location, String), WriteError> {
        let segments: Vec<&str> = path.segments().collect();
        for depth in (0..segments.len()).rev() {
            let ancestor: LogicalPath = format!("/{}", segments[..depth].join("/"))
                .parse()
                .map_err(|_| WriteError::Refused {
                    path: path.clone(),
                    code: Code::PathMalformed,
                })?;
            match self.resolve(&Source::WorkingTree, &ancestor) {
                Ok(location) => {
                    let directory = match location.object {
                        Object::Directory => location.path.clone(),
                        // A record gains children in a directory beside it.
                        Object::Blob(_) => match location.path.strip_suffix(".json") {
                            Some(stem) => stem.to_owned(),
                            None => {
                                return Err(WriteError::Refused {
                                    path: path.clone(),
                                    code: Code::RecordNotARecord,
                                });
                            }
                        },
                    };
                    let rest = &segments[depth..];
                    let mut physical: Vec<String> = Vec::new();
                    if !directory.is_empty() {
                        physical.push(directory);
                    }
                    physical.extend(rest.iter().map(|segment| (*segment).to_owned()));
                    let file = format!("{}.json", physical.join("/"));
                    return Ok((location, file));
                }
                Err(ResolveError::NotFound { .. }) => {}
                Err(error) => {
                    return Err(WriteError::Refused {
                        path: path.clone(),
                        code: error.code(),
                    });
                }
            }
        }
        Err(WriteError::Refused {
            path: path.clone(),
            code: Code::PathNotFound,
        })
    }

    /// The code the entry for `file` would have in the deepest directory of
    /// its path that exists, once written: the new file itself, or the first
    /// directory to be created on the way to it.
    fn degraded(&self, file: &str, is_pointer: bool) -> Result<Option<Code>, WriteError> {
        let mut known = |_: &str, _: &board_model::Stat| None;
        let mut tree = self.working_tree(&mut known)?;
        let mut directory = PathBuf::new();
        let mut parts = file.split('/').peekable();
        while let Some(part) = parts.next() {
            let last = parts.peek().is_none();
            let listed = tree.list(&directory)?;
            let exists = listed.iter().any(|entry| entry.name == part.as_bytes());
            if exists && !last {
                directory.push(part);
                continue;
            }
            let mut entries: Vec<RawEntry<PathBuf>> = listed
                .iter()
                .filter(|entry| entry.name != part.as_bytes())
                .cloned()
                .collect();
            entries.push(RawEntry {
                name: part.as_bytes().to_vec(),
                kind: if last {
                    Kind::File(ObjectId::Sha1([0; 20]))
                } else {
                    Kind::Directory(directory.join(part))
                },
                stat: None,
                size: None,
            });
            let ours = entries.len() - 1;
            let addressed = address::directory(&LogicalPath::root(), &entries, |record| {
                if record.name == part.as_bytes() {
                    Some(is_pointer)
                } else {
                    let read = tree.read(&directory, record).ok()?;
                    Some(
                        read.is_some_and(|bytes| !matches!(board_model::pointer(&bytes), Ok(None))),
                    )
                }
            });
            return Ok(addressed.get(ours).and_then(|(_, code)| *code));
        }
        Ok(None)
    }

    /// The bytes of the working-tree file at the `/`-separated `path`, at
    /// most one byte past [`MAX_RECORD_BYTES`] so a parse sees it is too
    /// large, or `None` when there is none.
    pub fn read_working_file(&self, path: &str) -> Result<Option<Vec<u8>>, WriteError> {
        let absolute = self.path().join(path);
        let read = fs::File::open(&absolute).and_then(|file| {
            let mut bytes = Vec::new();
            file.take(MAX_RECORD_BYTES as u64 + 1)
                .read_to_end(&mut bytes)?;
            Ok(bytes)
        });
        match read {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(WriteError::Read {
                path: absolute,
                source,
            }),
        }
    }

    /// The git directory of this working tree, where its locks live.
    pub fn git_dir(&self) -> &Path {
        self.git().git_dir()
    }

    /// Takes `board.lock` in the git directory, created exclusively as git
    /// creates its lock files, trying again until `wait` has passed.
    pub fn lock(&self, wait: Duration) -> Result<Lock, WriteError> {
        let path = self.git_dir().join("board.lock");
        let started = Instant::now();
        loop {
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(_) => return Ok(Lock { path }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    if started.elapsed() >= wait {
                        return Err(WriteError::Locked { lock: path });
                    }
                    std::thread::sleep(LOCK_POLL);
                }
                Err(source) => return Err(WriteError::Io { path, source }),
            }
        }
    }

    /// The blob of the working-tree file at the `/`-separated `path`, or
    /// `None` when there is none. Read in full: a put compares it.
    pub fn current_blob(&self, path: &str) -> Result<Option<ObjectId>, WriteError> {
        let absolute = self.path().join(path);
        match fs::read(&absolute) {
            Ok(bytes) => Ok(Some(self.blob_id(&bytes))),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(WriteError::Read {
                path: absolute,
                source,
            }),
        }
    }

    /// The id git gives a blob holding `bytes`.
    ///
    /// # Panics
    ///
    /// Never: hashing bytes in memory reads no stream.
    pub fn blob_id(&self, bytes: &[u8]) -> ObjectId {
        let id = gix::objs::compute_hash(self.git().object_hash(), gix::objs::Kind::Blob, bytes)
            .expect("hashing bytes in memory reads no stream, so it cannot fail");
        model_id(&id)
    }

    /// Writes `bytes` to the working-tree file at the `/`-separated `path`
    /// so that a kill leaves the old file or the new one: to a temporary
    /// file beside it, fsynced, renamed into place, and the directory
    /// fsynced. Missing directories are created; an existing file keeps its
    /// permissions. The lock must be held.
    pub fn write_file(&self, _lock: &Lock, path: &str, bytes: &[u8]) -> Result<(), WriteError> {
        let target = self.path().join(path);
        let io = |path: &Path| {
            let path = path.to_path_buf();
            move |source| WriteError::Io { path, source }
        };
        let directory = target.parent().unwrap_or(self.path()).to_path_buf();
        fs::create_dir_all(&directory).map_err(io(&directory))?;
        let name = target
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let temporary = directory.join(format!(".{name}.board-tmp"));
        let written = write_temporary(&temporary, &target, bytes)
            .and_then(|()| fs::rename(&temporary, &target));
        if let Err(source) = written {
            let _ = fs::remove_file(&temporary);
            return Err(WriteError::Io {
                path: target,
                source,
            });
        }
        fs::File::open(&directory)
            .and_then(|directory| directory.sync_all())
            .map_err(|source| WriteError::Unsynced {
                path: target,
                source,
            })
    }
}

/// A commit made ready before a put renames anything: the commit object is
/// written and git's `index.lock` is held, but HEAD has not moved. Dropping
/// it unfinished releases the index lock and changes nothing.
#[derive(Debug)]
pub struct Staged {
    commit: ObjectId,
    parent: Option<ObjectId>,
    index: gix::index::File,
    index_lock: PathBuf,
    finished: bool,
}

impl Staged {
    pub fn commit(&self) -> ObjectId {
        self.commit
    }
}

impl Drop for Staged {
    fn drop(&mut self) {
        if !self.finished {
            let _ = fs::remove_file(&self.index_lock);
        }
    }
}

impl Repository {
    /// Makes a commit of `writes`, each a `/`-separated path and the bytes it
    /// will hold, onto HEAD as `git commit <paths>` would, without moving
    /// HEAD: checks there is an identity to commit as, takes git's
    /// `index.lock` without waiting, and writes the blobs, trees and commit
    /// object. The board lock must be held, so HEAD is read under it.
    pub fn stage_commit(
        &self,
        _lock: &Lock,
        writes: &[(&str, &[u8])],
        message: &str,
    ) -> Result<Staged, WriteError> {
        let git = self.git();
        if !matches!(git.committer(), Some(Ok(_))) || !matches!(git.author(), Some(Ok(_))) {
            return Err(WriteError::NoIdentity);
        }
        let index_lock = self.git_dir().join("index.lock");
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&index_lock)
        {
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                return Err(WriteError::Locked { lock: index_lock });
            }
            Err(source) => {
                return Err(WriteError::Io {
                    path: index_lock,
                    source,
                });
            }
        }
        let mut staged = Staged {
            commit: ObjectId::Sha1([0; 20]),
            parent: self.head()?,
            index: self.load_index()?,
            index_lock,
            finished: false,
        };
        let failed =
            |source: Box<dyn std::error::Error + Send + Sync>| WriteError::Commit { source };
        let base = match staged.parent {
            Some(parent) => self.commit_tree(parent)?.root(),
            None => model_id(&gix::ObjectId::empty_tree(git.object_hash())),
        };
        let base =
            gix::ObjectId::try_from(base.as_bytes()).map_err(|error| failed(error.into()))?;
        let mut editor = git.edit_tree(base).map_err(|error| failed(error.into()))?;
        for (path, bytes) in writes {
            let blob = git
                .write_blob(bytes)
                .map_err(|error| failed(error.into()))?
                .detach();
            editor
                .upsert(*path, gix::object::tree::EntryKind::Blob, blob)
                .map_err(|error| failed(error.into()))?;
            stage_entry(&mut staged.index, path, blob);
        }
        let tree = editor
            .write()
            .map_err(|error| failed(error.into()))?
            .detach();
        let parents: Vec<gix::ObjectId> = staged
            .parent
            .iter()
            .filter_map(|parent| gix::ObjectId::try_from(parent.as_bytes()).ok())
            .collect();
        let commit = git
            .new_commit(message, tree, parents)
            .map_err(|error| failed(error.into()))?;
        staged.commit = model_id(&commit.id);
        Ok(staged)
    }

    /// Finishes `staged` once its files are in place: moves HEAD to the
    /// commit only if HEAD still holds the commit read when it was staged,
    /// then writes git's index for the committed paths. If HEAD moved, the
    /// index is left as it was and the put's files stay uncommitted edits.
    pub fn finish_commit(&self, mut staged: Staged) -> Result<ObjectId, WriteError> {
        use gix::refs::Target as RefTarget;
        use gix::refs::transaction::{Change, LogChange, PreviousValue, RefEdit, RefLog};
        let commit = gix::ObjectId::try_from(staged.commit.as_bytes()).map_err(|error| {
            WriteError::Commit {
                source: error.into(),
            }
        })?;
        let expected = match staged.parent {
            Some(parent) => PreviousValue::MustExistAndMatch(RefTarget::Object(
                gix::ObjectId::try_from(parent.as_bytes()).map_err(|error| WriteError::Commit {
                    source: error.into(),
                })?,
            )),
            None => PreviousValue::MustNotExist,
        };
        let edit =
            RefEdit {
                change: Change::Update {
                    log: LogChange {
                        mode: RefLog::AndReference,
                        force_create_reflog: false,
                        message: "commit: board put".into(),
                    },
                    expected,
                    new: RefTarget::Object(commit),
                },
                name: "HEAD".try_into().map_err(
                    |error: gix::validate::reference::name::Error| WriteError::Commit {
                        source: error.into(),
                    },
                )?,
                deref: true,
            };
        if self.git().edit_reference(edit).is_err() {
            return Err(WriteError::HeadMoved);
        }
        let index_path = self.git_dir().join("index");
        let written = OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&staged.index_lock)
            .and_then(|mut file| {
                staged
                    .index
                    .write_to(&mut file, gix::index::write::Options::default())
                    .map_err(io::Error::other)?;
                file.sync_all()
            })
            .and_then(|()| fs::rename(&staged.index_lock, &index_path));
        written.map_err(|source| WriteError::Io {
            path: index_path,
            source,
        })?;
        staged.finished = true;
        Ok(staged.commit)
    }

    /// Git's index for this working tree, or an empty one before the first
    /// `git add`.
    fn load_index(&self) -> Result<gix::index::File, WriteError> {
        let git = self.git();
        if git.index_path().exists() {
            git.open_index().map_err(|error| WriteError::Commit {
                source: error.into(),
            })
        } else {
            Ok(gix::index::File::from_state(
                gix::index::State::new(git.object_hash()),
                git.index_path(),
            ))
        }
    }
}

/// Points the index entry for `path` at `blob`, adding it when new. Its
/// stat is cleared, so git hashes the file again before trusting it.
fn stage_entry(index: &mut gix::index::File, path: &str, blob: gix::ObjectId) {
    use gix::bstr::ByteSlice;
    index.remove_tree();
    if let Ok(at) = index.entry_index_by_path(path.as_bytes().as_bstr()) {
        let entry = &mut index.entries_mut()[at];
        entry.id = blob;
        entry.stat = gix::index::entry::Stat::default();
    } else {
        index.dangerously_push_entry(
            gix::index::entry::Stat::default(),
            blob,
            gix::index::entry::Flags::empty(),
            gix::index::entry::Mode::FILE,
            path.as_bytes().as_bstr(),
        );
        index.sort_entries();
    }
}

/// Writes and fsyncs the temporary file, with the target's permissions when
/// the target exists.
fn write_temporary(temporary: &Path, target: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(temporary)?;
    file.write_all(bytes)?;
    if let Ok(metadata) = fs::metadata(target) {
        file.set_permissions(metadata.permissions())?;
    }
    file.sync_all()
}

/// The record file a resolved Location writes to: the file itself, or the
/// record beside a directory. `None` when it is a file that is not a record,
/// or the root.
fn record_file(location: &Location) -> Option<String> {
    match location.object {
        Object::Blob(_) => board_model::needs_bytes(&location.path).then(|| location.path.clone()),
        Object::Directory if location.path.is_empty() => None,
        Object::Directory => Some(format!("{}.json", location.path)),
    }
}

/// A write that cannot be made.
#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    #[error("{path} cannot be written: {code}")]
    Refused { path: LogicalPath, code: Code },
    #[error("{lock:?} is held; if no put is running, remove it")]
    Locked { lock: PathBuf },
    #[error("{path:?} cannot be read")]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("{path:?} could not be written")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("git has no user.name and user.email to commit as")]
    NoIdentity,
    #[error("HEAD moved while the put ran; its files are written but not committed")]
    HeadMoved,
    #[error("the commit cannot be made")]
    Commit {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[error(
        "{path:?} may already be in place, but its directory could not be synced, so a crash could lose it"
    )]
    Unsynced {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error(transparent)]
    Git(#[from] GitError),
}

impl WriteError {
    pub fn code(&self) -> Code {
        match self {
            Self::Refused { code, .. } => *code,
            Self::Locked { .. } => Code::PutLocked,
            Self::Read { .. } => Code::RepoUnreadable,
            Self::Io { .. } | Self::Unsynced { .. } | Self::Commit { .. } => Code::PutFailed,
            Self::NoIdentity => Code::PutNoIdentity,
            Self::HeadMoved => Code::PutHeadMoved,
            Self::Git(error) => error.code(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failure() -> io::Error {
        io::Error::other("the disk is full")
    }

    #[test]
    fn a_write_that_fails_is_put_failed_and_a_read_that_fails_is_unreadable() {
        let path = PathBuf::from("/board/tasks/a.json");
        let write = WriteError::Io {
            path: path.clone(),
            source: failure(),
        };
        let unsynced = WriteError::Unsynced {
            path: path.clone(),
            source: failure(),
        };
        let read = WriteError::Read {
            path,
            source: failure(),
        };
        assert_eq!(write.code(), Code::PutFailed);
        assert_eq!(unsynced.code(), Code::PutFailed);
        assert!(
            unsynced.to_string().contains("may already be in place"),
            "nobody may read it as nothing written"
        );
        assert_eq!(read.code(), Code::RepoUnreadable);
    }
}
