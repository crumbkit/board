//! The one module that writes the working tree and the git directory
//! (docs/architecture.md, Writes; docs/architecture/paths/writes.md): where
//! a put lands, the lock, and the write that a kill cannot leave half done.
#![expect(
    clippy::disallowed_methods,
    reason = "board-tree's write module owns every write to the working tree and the git directory"
)]

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use board_model::{BINARY_NAME, Code, Fields, Location, LogicalPath, Object, ObjectId, Source};
use gix::lock::acquire::Fail;

use crate::git::{GitError, Repository, model_id};
use crate::repositories::Repositories;
use crate::resolve::{ResolveError, Resolver};
use crate::through::{Inside, Through};
use crate::tree::Tree;
use crate::walk::{Beneath, Planned, Tense, reason};
use entries::{stage_entry, unstage_entry};

mod entries;
pub(crate) mod outside;
pub(crate) mod sync;

/// How long a put waits for another put's lock.
pub const LOCK_WAIT: Duration = Duration::from_secs(5);
/// How often a waiting put tries the lock again.
const LOCK_POLL: Duration = Duration::from_millis(20);

/// Where a put writes: the record file a logical path names, or the file
/// whose whole name it is, existing or not, in the working tree.
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
    _held: gix::lock::Marker,
}

impl Repositories {
    /// The record file `path` names for writing, as [`Resolver::target`]
    /// finds it in a snapshot of its own.
    pub fn target(&self, path: &LogicalPath, is_pointer: bool) -> Result<Target, WriteError> {
        self.snapshot()?.target(path, is_pointer)
    }

    /// The pointer file `path` names, as [`Resolver::pointer_target`] finds
    /// it in a snapshot of its own.
    pub fn pointer_target(&self, path: &LogicalPath) -> Result<Target, WriteError> {
        self.snapshot()?.pointer_target(path)
    }
}

impl Resolver<'_, '_> {
    /// The record file `path` names for writing. Pointers are followed,
    /// including a link at the last segment; a path that reaches a mount is
    /// refused. A path that names nothing yet names a new file beneath the
    /// deepest directory that exists. `is_pointer` says whether the record
    /// to be written is a pointer, which would shadow a directory beside it;
    /// a write that would leave its own path degraded is refused with that
    /// path's code.
    pub fn target(&mut self, path: &LogicalPath, is_pointer: bool) -> Result<Target, WriteError> {
        let (location, physical, reached) = match self.resolve(path) {
            Ok(location) => {
                let physical = record_file(&location);
                (location, physical, path.clone())
            }
            Err(ResolveError::NotFound { .. }) => self.new_file(path)?,
            Err(source) => return Err(unresolved(path, source)),
        };
        if location.board.source != Source::WorkingTree {
            return Err(through_mount(path, &location, physical.as_deref(), ".json"));
        }
        // The walk lists a symlink as a file, but on disk it can lead to a
        // directory that a write beneath it would go into.
        let Some(physical) = physical else {
            self.repository().refuse_symlink(path, &location.path)?;
            return Err(not_a_record(path, reached));
        };
        self.repository().refuse_symlink(path, &physical)?;
        self.writable(path, physical, location, is_pointer)
    }

    /// The file whose whole name is `path`, for writing bytes as given.
    /// Pointers are followed as [`Resolver::target`] follows them. A path
    /// that names a directory, or a JSON file, which is read as a record,
    /// is refused.
    pub fn file_target(&mut self, path: &LogicalPath) -> Result<Target, WriteError> {
        let (location, physical, reached) = match self.resolve(path) {
            Ok(location) => {
                let physical = match location.object {
                    Object::Blob(_) => Some(location.path.clone()),
                    Object::Directory => None,
                };
                (location, physical, path.clone())
            }
            Err(ResolveError::NotFound { .. }) => {
                let ancestor = self
                    .ancestor(path)
                    .map_err(|source| unresolved(path, source))?;
                let physical = ancestor.path();
                (ancestor.location, physical, ancestor.logical)
            }
            Err(source) => return Err(unresolved(path, source)),
        };
        if location.board.source != Source::WorkingTree {
            return Err(through_mount(path, &location, physical.as_deref(), ""));
        }
        let Some(physical) = physical else {
            // A directory holds no bytes, and a path continuing through a
            // file other than a record names nothing.
            return Err(match location.object {
                Object::Directory => WriteError::NotAFile {
                    path: path.clone(),
                    file: None,
                },
                Object::Blob(_) => not_a_record(path, reached),
            });
        };
        if board_model::is_json_file(&physical) {
            return Err(WriteError::NotAFile {
                path: path.clone(),
                file: Some(physical),
            });
        }
        self.repository().refuse_symlink(path, &physical)?;
        self.writable(path, physical, location, false)
    }

    /// The pointer file `path` names, which is not followed, for rewriting
    /// in place. A path that reaches a mount on the way is refused, as is
    /// one that names no file or a pointer left degraded.
    pub fn pointer_target(&mut self, path: &LogicalPath) -> Result<Target, WriteError> {
        let location = self
            .resolve_pointer(path)
            .map_err(|source| unresolved(path, source))?;
        if location.board.source != Source::WorkingTree {
            return Err(through_mount(path, &location, None, ""));
        }
        let Object::Blob(_) = location.object else {
            return Err(not_a_record(path, path.clone()));
        };
        let physical = record_file(&location).ok_or_else(|| not_a_record(path, path.clone()))?;
        self.repository().refuse_symlink(path, &physical)?;
        self.writable(path, physical, location, true)
    }

    /// The target `physical` names, unless it names git's own directory or
    /// the walk would degrade it once written: with its own code, or, when
    /// the code is a directory's, naming that directory.
    fn writable(
        &mut self,
        path: &LogicalPath,
        physical: String,
        location: Location,
        is_pointer: bool,
    ) -> Result<Target, WriteError> {
        let path = path.clone();
        if names_git(&physical) {
            return Err(WriteError::InGit {
                path,
                file: physical,
            });
        }
        match self.planned_code(&physical, is_pointer)? {
            None => Ok(Target {
                path: physical,
                location,
            }),
            Some(Planned::File(code)) => Err(WriteError::Degraded { path, code }),
            Some(Planned::Directory(beneath)) => Err(WriteError::Beneath { path, beneath }),
        }
    }

    /// The new record file `path` names: beneath its deepest ancestor that
    /// resolves, with the segments past it as directories to create. `None`
    /// when the ancestor is a file other than a record, which holds nothing;
    /// the ancestor's own logical path comes last.
    fn new_file(
        &mut self,
        path: &LogicalPath,
    ) -> Result<(Location, Option<String>, LogicalPath), WriteError> {
        let ancestor = self
            .ancestor(path)
            .map_err(|source| unresolved(path, source))?;
        let file = ancestor.path().map(|file| format!("{file}.json"));
        Ok((ancestor.location, file, ancestor.logical))
    }
}

/// A write to `path` refused because resolving it failed. A symlink that
/// leads to no file is refused as any symlink is.
fn unresolved(path: &LogicalPath, source: ResolveError) -> WriteError {
    match source {
        ResolveError::Symlink { link, .. } => WriteError::Symlink {
            path: path.clone(),
            symlink: link.into(),
        },
        source => WriteError::Unresolved {
            path: path.clone(),
            source,
        },
    }
}

/// Why resolving `path` failed, for a message that has already named it:
/// an error said of `path` itself calls it `it`.
fn said(path: &LogicalPath, source: &ResolveError) -> String {
    match source.predicate() {
        Some(predicate) if source.fields() == Fields::at(path) => format!("it {predicate}"),
        _ => source.to_string(),
    }
}

/// A write to `path` refused because `location`, where it leads, is in a
/// mounted board, naming the last pointer in the working tree: the mount
/// that leads out of it. Through one mount to the file `physical`, whose
/// logical path drops `suffix`, it says where that file is in the mounted
/// board, for a put to write.
fn through_mount(
    path: &LogicalPath,
    location: &Location,
    physical: Option<&str>,
    suffix: &str,
) -> WriteError {
    let pointers = location.pointers.iter();
    let mount = pointers
        .clone()
        .take_while(|pointer| pointer.board.source == Source::WorkingTree)
        .last()
        .and_then(Location::display_path);
    let mut mounted = pointers.filter(|pointer| pointer.board.source != Source::WorkingTree);
    let one = mounted.all(|pointer| pointer.board == location.board);
    let why = if one {
        Through::Mounted
    } else {
        Through::Nested
    };
    let inside = match (&location.board.source, physical) {
        (Source::Commit(commit), Some(file)) if one => file
            .strip_suffix(suffix)
            .and_then(|logical| format!("/{logical}").parse().ok())
            .map(|inside| {
                Box::new(Inside {
                    repository: location.board.repository.clone(),
                    commit: *commit,
                    file: file.to_owned(),
                    path: inside,
                })
            }),
        _ => None,
    };
    // A mount file with no logical path, such as one reached through a
    // symlink by a path longer than a logical path may be, has no
    // `display_path`; the error then names `path`.
    WriteError::ThroughMount {
        path: path.clone(),
        mount: mount.unwrap_or_else(|| path.clone()),
        why: Box::new(why),
        inside,
    }
}

/// A write to `path` refused because it reaches `file`, which is not a
/// record.
fn not_a_record(path: &LogicalPath, file: LogicalPath) -> WriteError {
    WriteError::NotARecord {
        path: path.clone(),
        file,
    }
}

impl Repository {
    /// Refuses a write to the `/`-separated `file` when it, or a directory
    /// on the way to it, is a symlink. The working-tree listing hides an
    /// ignored symlink, so resolution alone would write through one.
    fn refuse_symlink(&self, path: &LogicalPath, file: &str) -> Result<(), WriteError> {
        match self.symlink_on_way(file)? {
            Some(symlink) => Err(WriteError::Symlink {
                path: path.clone(),
                symlink,
            }),
            None => Ok(()),
        }
    }

    /// The git directory of this working tree, where its locks live.
    pub fn git_dir(&self) -> &Path {
        self.git().git_dir()
    }

    /// Takes `board.lock` in the git directory with gix's lock files, trying
    /// again every `LOCK_POLL` until `wait` has passed.
    pub fn lock(&self, wait: Duration) -> Result<Lock, WriteError> {
        let resource = self.git_dir().join("board");
        let started = Instant::now();
        loop {
            match gix::lock::Marker::acquire_to_hold_resource(&resource, Fail::Immediately, None) {
                Ok(held) => return Ok(Lock { _held: held }),
                Err(error) if error.is_retryable() && started.elapsed() < wait => {
                    std::thread::sleep(LOCK_POLL);
                }
                Err(error) => return Err(lock_refused(error, resource.with_extension("lock"))),
            }
        }
    }

    /// The id git gives a blob holding `bytes`.
    pub fn blob_id(&self, bytes: &[u8]) -> ObjectId {
        blob_id(self.git().object_hash(), bytes)
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

    /// Removes the working-tree file at the `/`-separated `path`, and is
    /// done when it is already gone. The lock must be held.
    pub fn remove_file(&self, _lock: &Lock, path: &str) -> Result<(), WriteError> {
        let target = self.path().join(path);
        match fs::remove_file(&target) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(WriteError::Io {
                path: target,
                source,
            }),
        }
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
    index_lock: gix::lock::File,
}

/// The id git gives a blob holding `bytes` under the object hash `kind`.
///
/// # Panics
///
/// Never: hashing bytes in memory reads no stream.
pub(crate) fn blob_id(kind: gix::hash::Kind, bytes: &[u8]) -> ObjectId {
    let id = gix::objs::compute_hash(kind, gix::objs::Kind::Blob, bytes)
        .expect("hashing bytes in memory reads no stream, so it cannot fail");
    model_id(&id)
}

impl Staged {
    pub fn commit(&self) -> ObjectId {
        self.commit
    }
}

impl Repository {
    /// Makes a commit of `writes`, each a `/`-separated path and the bytes it
    /// will hold, or `None` when it is removed, onto HEAD as `git commit
    /// <paths>` would, without moving
    /// HEAD: checks there is an identity to commit as, takes git's
    /// `index.lock` without waiting, and writes the blobs, trees and commit
    /// object. The board lock must be held, so HEAD is read under it.
    /// `None` when HEAD already holds every write, as git commits nothing.
    pub fn stage_commit(
        &self,
        _lock: &Lock,
        writes: &[(&str, Option<&[u8]>)],
        message: &str,
    ) -> Result<Option<Staged>, WriteError> {
        use gix::state::InProgress as Op;
        let git = self.git();
        if !matches!(git.committer(), Some(Ok(_))) || !matches!(git.author(), Some(Ok(_))) {
            return Err(WriteError::NoIdentity);
        }
        // As `git commit <paths>`, which refuses a partial commit while an
        // operation that would give the commit more parents is under way.
        let operation = match git.state() {
            None | Some(Op::Bisect) => None,
            Some(Op::Merge) => Some("merge"),
            Some(Op::CherryPick | Op::CherryPickSequence) => Some("cherry-pick"),
            Some(Op::Revert | Op::RevertSequence) => Some("revert"),
            Some(Op::Rebase | Op::RebaseInteractive) => Some("rebase"),
            Some(Op::ApplyMailbox | Op::ApplyMailboxRebase) => Some("rebase or am"),
        };
        if let Some(operation) = operation {
            return Err(WriteError::InProgress { operation });
        }
        let mut staged = Staged {
            commit: ObjectId::Sha1([0; 20]),
            parent: self.head()?,
            index: self.load_index()?,
            index_lock: self.lock_index()?,
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
            let Some(bytes) = bytes else {
                editor
                    .remove_leaf(*path)
                    .map_err(|error| failed(error.into()))?;
                unstage_entry(&mut staged.index, path);
                continue;
            };
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
        if tree == base {
            return Ok(None);
        }
        let parents: Vec<gix::ObjectId> = staged
            .parent
            .iter()
            .filter_map(|parent| gix::ObjectId::try_from(parent.as_bytes()).ok())
            .collect();
        let commit = git
            .new_commit(message, tree, parents)
            .map_err(|error| failed(error.into()))?;
        staged.commit = model_id(&commit.id);
        Ok(Some(staged))
    }

    /// Finishes `staged` once its files are in place: moves HEAD to the
    /// commit only if HEAD still holds the commit read when it was staged,
    /// then writes git's index for the committed paths. If HEAD moved, the
    /// index is left as it was and the put's files stay uncommitted edits.
    /// The board lock taken before staging must still be held.
    pub fn finish_commit(&self, lock: &Lock, staged: Staged) -> Result<ObjectId, WriteError> {
        self.finish(lock, staged, &format!("commit: {BINARY_NAME} put"))
    }

    /// Finishes `staged` as [`Self::finish_commit`] does, writing `log` to
    /// the reflog.
    fn finish(&self, _lock: &Lock, mut staged: Staged, log: &str) -> Result<ObjectId, WriteError> {
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
                        message: log.into(),
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
        // As git does, the reflog names a fallback identity when none is
        // configured: a put has refused by then, and a sync that only
        // fast-forwards commits nothing.
        let mut git = self.git().clone();
        git.committer_or_set_generic_fallback()
            .map_err(|error| WriteError::Uncommitted {
                source: error.into(),
            })?;
        if let Err(error) = git.edit_reference(edit) {
            use gix::refs::file::transaction::prepare::{MustNotExist, ReferenceOutOfDate};
            if error.downcast_any_ref::<ReferenceOutOfDate>().is_some()
                || error.downcast_any_ref::<MustNotExist>().is_some()
            {
                return Err(WriteError::HeadMoved);
            }
            return Err(WriteError::Uncommitted {
                source: error.into(),
            });
        }
        let index = &staged.index;
        staged
            .index_lock
            .with_mut(|file| {
                index
                    .write_to(&mut *file, gix::index::write::Options::default())
                    .map_err(io::Error::other)?;
                file.sync_all()
            })
            .and_then(|()| staged.index_lock.commit().map_err(|error| error.error))
            .map_err(|source| WriteError::IndexNotUpdated {
                commit: staged.commit,
                source,
            })?;
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

    /// Takes git's `index.lock` without waiting.
    fn lock_index(&self) -> Result<gix::lock::File, WriteError> {
        let index = self.git().index_path();
        gix::lock::File::acquire_to_update_resource(&index, Fail::Immediately, None)
            .map_err(|error| lock_refused(error, index.with_extension("lock")))
    }
}

/// Writes and fsyncs the temporary file, with the target's permissions when
/// the target exists. A symlink at `temporary`, which a commit can plant,
/// is removed first so the write never follows it. Soft limit: a symlink
/// made between that check and the open is still followed; opening with
/// `O_NOFOLLOW` would close that, at the cost of rustix in this crate.
/// Anything else there is reused, which the kill test's FIFO relies on.
fn write_temporary(temporary: &Path, target: &Path, bytes: &[u8]) -> io::Result<()> {
    if fs::symlink_metadata(temporary).is_ok_and(|metadata| metadata.is_symlink()) {
        fs::remove_file(temporary)?;
    }
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

/// The refusal for a lock gix could not take: `Locked` when another holds
/// `lock`, and an I/O failure otherwise.
fn lock_refused(error: gix::Exn, lock: PathBuf) -> WriteError {
    if error.is_retryable() {
        WriteError::Locked { lock }
    } else {
        WriteError::Io {
            path: lock,
            source: io::Error::other(error.into_error()),
        }
    }
}

/// The record file a resolved Location writes to: the file itself, or the
/// record beside a directory. `None` when it is a file that is not a record,
/// or the root.
fn record_file(location: &Location) -> Option<String> {
    match location.object {
        Object::Blob(_) => board_model::is_json_file(&location.path).then(|| location.path.clone()),
        Object::Directory if location.path.is_empty() => None,
        Object::Directory => Some(format!("{}.json", location.path)),
    }
}

/// Whether a segment of the `/`-separated `file` is one git keeps for
/// itself: `.git` under any spelling that HFS+ or NTFS opens as it, such as
/// `.GIT` or `GIT~1`.
fn names_git(file: &str) -> bool {
    use gix::validate::path::component;
    let options = component::Options {
        protect_windows: false,
        protect_hfs: true,
        protect_ntfs: true,
    };
    file.split('/')
        .any(|segment| gix::validate::path::component(segment.into(), None, options).is_err())
}

/// A write that cannot be made.
#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    #[error("{path} cannot be written: {}", said(path, source))]
    Unresolved {
        path: LogicalPath,
        #[source]
        source: ResolveError,
    },
    /// `mount` is the mount in the working tree that `path` passes through;
    /// `inside` is where one mount leads, for a put to write there.
    #[error("{path} cannot be written: it is in the board mounted at {mount}{why}")]
    ThroughMount {
        path: LogicalPath,
        mount: LogicalPath,
        why: Box<Through>,
        inside: Option<Box<Inside>>,
    },
    /// `file` is what the path reaches: itself, or its deepest ancestor.
    #[error("{path} cannot be written: {file} is not a record")]
    NotARecord {
        path: LogicalPath,
        file: LogicalPath,
    },
    /// `file` is the physical path from the board's root.
    #[error("{path} cannot be written: {file} would be in git's own directory")]
    InGit { path: LogicalPath, file: String },
    /// A file put refused because `path` names `file`, a JSON file read as
    /// a record, or with no `file`, a directory.
    #[error("{path} cannot hold a file: {}", file.as_ref().map_or_else(|| "it is a directory".to_owned(), |file| format!("{file} is read as a record")))]
    NotAFile {
        path: LogicalPath,
        file: Option<String>,
    },
    /// Refused for the code the file itself would have once written.
    #[error("{path} cannot be written: it {}", reason(*code, Tense::Would, None))]
    Degraded { path: LogicalPath, code: Code },
    /// Refused for the code of a directory the file would be in.
    #[error("{path} cannot be written: {beneath}")]
    Beneath { path: LogicalPath, beneath: Beneath },
    #[error("{path} cannot be written: {symlink} is a symlink")]
    Symlink { path: LogicalPath, symlink: String },
    #[error("{lock:?} is held; if no put is running, remove it")]
    Locked { lock: PathBuf },
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
    #[error("HEAD could not be moved, so the files are written but not committed: {source}")]
    Uncommitted {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[error(
        "committed {commit}, but git's index was not updated, so git status shows the written files as changed: {source}"
    )]
    IndexNotUpdated {
        commit: ObjectId,
        #[source]
        source: io::Error,
    },
    #[error("git is in the middle of a {operation}; finish or abort it before a put commits")]
    InProgress { operation: &'static str },
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
            Self::Unresolved { source, .. } => source.code(),
            Self::ThroughMount { why, .. } => why.code(),
            Self::NotARecord { .. } => Code::RecordNotARecord,
            Self::InGit { .. } => Code::PathMalformed,
            Self::NotAFile { .. } => Code::PutNotAFile,
            Self::Degraded { code, .. } => *code,
            Self::Beneath { beneath, .. } => beneath.code,
            Self::Symlink { .. } => Code::PutSymlink,
            Self::Locked { .. } | Self::InProgress { .. } => Code::PutLocked,
            Self::Io { .. }
            | Self::Unsynced { .. }
            | Self::Commit { .. }
            | Self::Uncommitted { .. }
            | Self::IndexNotUpdated { .. } => Code::PutFailed,
            Self::NoIdentity => Code::PutNoIdentity,
            Self::HeadMoved => Code::PutHeadMoved,
            Self::Git(error) => error.code(),
        }
    }

    pub fn fields(&self) -> Fields {
        match self {
            Self::Unresolved {
                source: ResolveError::Git(_),
                path,
            }
            | Self::ThroughMount { mount: path, .. }
            | Self::InGit { path, .. }
            | Self::NotAFile { path, .. }
            | Self::Degraded { path, .. }
            | Self::Symlink { path, .. } => Fields::at(path),
            Self::Unresolved { source, .. } => source.fields(),
            Self::NotARecord { file, .. } => Fields::at(file),
            Self::Beneath { path, beneath } => Fields {
                directory: Some(beneath.directory.clone()),
                ..Fields::at(path)
            },
            Self::Git(error) => error.fields(),
            _ => Fields::default(),
        }
    }
}

#[cfg(test)]
mod tests;
