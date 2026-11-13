//! `board sync`: fetching the branch HEAD follows, replaying the commits
//! only this clone has onto what was fetched, pushing the result, and
//! moving HEAD, the index and the working tree to it (docs/architecture.md,
//! Writes). The lock the caller holds keeps puts out while it runs.

use std::collections::BTreeMap;
use std::io;
use std::time::Duration;

use board_model::{Code, Fields, LogicalPath, ObjectId};
use gix::bstr::{BString, ByteSlice};
use gix::object::tree::EntryKind;

use super::entries::{stage_entry, unstage_entry};
use super::{LOCK_WAIT, Lock, Staged, WriteError};
use crate::git::{GitError, Repository, model_id};
use crate::transport::{self, Deadline, Pushed, Upstream};

/// How many times a sync fetches and pushes before it gives up.
pub const ATTEMPTS: u32 = 3;

/// What a sync did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Synced {
    pub upstream: Upstream,
    /// The remote branch's commit after the last fetch.
    pub fetched: ObjectId,
    /// HEAD before, or `None` for an unborn branch.
    pub old: Option<ObjectId>,
    /// HEAD after: the fetched commit, or the last commit replayed onto it.
    pub new: ObjectId,
    /// Each commit only this clone had, oldest first, and what it became.
    pub replayed: Vec<Replayed>,
    pub pushed: bool,
    pub attempts: u32,
    /// Each file edited during the sync, kept without the remote's change.
    pub kept: Vec<Fields>,
}

/// A commit replayed onto the fetched branch: `to` is its new id, or
/// `None` when the remote already held every change it made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Replayed {
    pub from: ObjectId,
    pub to: Option<ObjectId>,
}

/// What one integrate did, or the rounds of a sync added together.
struct Integrated {
    old: Option<ObjectId>,
    new: ObjectId,
    replayed: Vec<Replayed>,
    kept: Vec<Fields>,
}

impl Integrated {
    /// This round followed by `next`, which replayed the commits this round
    /// left: each commit `next` replayed that this round made is traced
    /// back to the commit it came from.
    fn then(mut self, next: Self) -> Self {
        for replay in next.replayed {
            match self.replayed.iter_mut().find(|r| r.to == Some(replay.from)) {
                Some(earlier) => earlier.to = replay.to,
                None => self.replayed.push(replay),
            }
        }
        self.new = next.new;
        self.kept.extend(next.kept);
        self
    }
}

/// A tree's files by `/`-separated path, each with its object and kind.
type Files = BTreeMap<BString, (gix::ObjectId, EntryKind)>;

impl Repository {
    /// The remote branch that the branch HEAD is on follows, from git's
    /// `branch.<name>.remote` and `branch.<name>.merge`.
    pub fn upstream(&self) -> Result<Upstream, SyncError> {
        use gix::remote::Direction::Fetch;
        let git = self.git();
        let name = git
            .head_name()
            .map_err(|source| self.unreadable(source))?
            .ok_or(SyncError::NoRemote { branch: None })?;
        let short = name.shorten().to_string();
        let no_remote = || SyncError::NoRemote {
            branch: Some(short.clone()),
        };
        let remote = git
            .branch_remote_name(name.shorten(), Fetch)
            .ok_or_else(no_remote)?
            .as_bstr()
            .to_string();
        let merged = git
            .branch_remote_ref_name(name.as_ref(), Fetch)
            .ok_or_else(no_remote)?
            .map_err(|source| self.unreadable(source))?;
        let branch = merged.shorten().to_string();
        let tracking = git
            .branch_remote_tracking_ref_name(name.as_ref(), Fetch)
            .ok_or_else(no_remote)?
            .map_err(|source| self.unreadable(source))?;
        Ok(Upstream {
            remote,
            branch,
            tracking: tracking.to_string(),
        })
    }

    /// Brings this clone up to date with the upstream in rounds of fetch,
    /// integrate and, with `push`, push, up to [`ATTEMPTS`] rounds.
    /// Fetch and push run without the board lock, so puts continue while
    /// they wait on the network. Integrate takes the board lock for its
    /// local step only: it replays the commits only this clone has onto the
    /// fetched commit, then HEAD, the index and the working tree move to
    /// the result. A rejected push, or HEAD moved by another program during
    /// integrate, starts the next round from the fetch. A file the remote
    /// changed that is edited here stops the sync before it writes. One
    /// sync runs per clone: `sync.lock` refuses a second with
    /// [`SyncError::Busy`] once `wait` has passed; zero refuses at once. Every fetch, push and wait for the board lock
    /// shares `deadline`, and the git run at it is stopped.
    pub fn sync(
        &self,
        push: bool,
        deadline: Deadline,
        wait: Duration,
    ) -> Result<Synced, SyncError> {
        let upstream = self.upstream()?;
        let _only = self.sync_lock(wait.min(deadline.left()))?;
        let ssh = self.ssh();
        let mut summary: Option<Integrated> = None;
        let mut reason = String::new();
        for attempts in 1..=ATTEMPTS {
            transport::fetch(self.path(), &upstream, &ssh, deadline)?;
            let fetched = self.tracking(&upstream)?;
            let round = match self.integrate(fetched, deadline) {
                Ok(round) => round,
                Err(SyncError::Write(WriteError::HeadMoved)) if attempts < ATTEMPTS => continue,
                // A board lock still held when the deadline is spent is the
                // deadline running out, not a put in the way.
                Err(SyncError::Write(error))
                    if error.code() == Code::PutLocked && deadline.left().is_zero() =>
                {
                    return Err(SyncError::TimedOut {
                        operation: "integrate",
                        remote: upstream.remote.clone(),
                        left: Duration::ZERO,
                        deadline: deadline.given,
                    });
                }
                Err(error) => return Err(error),
            };
            let new = round.new;
            let total = match summary.take() {
                Some(before) => before.then(round),
                None => round,
            };
            let done = |pushed, total: Integrated| Synced {
                upstream: upstream.clone(),
                fetched,
                old: total.old,
                new,
                replayed: total.replayed,
                pushed,
                attempts,
                kept: total.kept,
            };
            if !push || new == fetched {
                return Ok(done(false, total));
            }
            match transport::push(self.path(), &upstream, &new.to_string(), &ssh, deadline)? {
                Pushed::Done => return Ok(done(true, total)),
                Pushed::Rejected(why) => reason = why,
            }
            summary = Some(total);
        }
        Err(SyncError::GaveUp {
            attempts: ATTEMPTS,
            reason,
        })
    }

    /// Takes an exclusive file lock on `sync.lock` in the git directory, which
    /// admits one sync at a time, trying again until `wait` has passed, then
    /// refuses with [`SyncError::Busy`]. The kernel drops the lock when the
    /// process ends, however it ends, so a file left behind by a killed sync
    /// does not hold anything.
    fn sync_lock(&self, wait: Duration) -> Result<std::fs::File, SyncError> {
        let lock = self.git_dir().join("sync.lock");
        let failed = |source| {
            SyncError::Write(WriteError::Io {
                path: lock.clone(),
                source,
            })
        };
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&lock)
            .map_err(failed)?;
        let started = std::time::Instant::now();
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(file),
                Err(std::fs::TryLockError::WouldBlock) if started.elapsed() < wait => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(std::fs::TryLockError::WouldBlock) => return Err(SyncError::Busy { lock }),
                Err(std::fs::TryLockError::Error(source)) => return Err(failed(source)),
            }
        }
    }

    /// The local step of a sync, under the board lock: replays the commits
    /// only this clone has onto `fetched`, then moves HEAD, the index and
    /// the working tree to the result. The lock is not held across any
    /// network call.
    fn integrate(&self, fetched: ObjectId, deadline: Deadline) -> Result<Integrated, SyncError> {
        let lock = self
            .lock(deadline.left().min(LOCK_WAIT))
            .map_err(SyncError::from)?;
        let old = self.head()?;
        let (new, replayed) = self.replay(old, fetched)?;
        let kept = if old == Some(new) {
            Vec::new()
        } else {
            let changes = self.changes(&self.files_of(old)?, &self.files_of(Some(new))?)?;
            self.check_out(&lock, old, new, changes)?
        };
        Ok(Integrated {
            old,
            new,
            replayed,
            kept,
        })
    }

    /// The commit `upstream`'s tracking ref names after a fetch.
    fn tracking(&self, upstream: &Upstream) -> Result<ObjectId, SyncError> {
        let mut found = self
            .git()
            .find_reference(upstream.tracking.as_str())
            .map_err(|source| self.unreadable(source))?;
        let id = found
            .peel_to_id()
            .map_err(|source| self.unreadable(source))?;
        Ok(model_id(&id))
    }

    /// The commits reachable from `old` but not from `tip`, replayed onto
    /// `tip` one by one as commits of the same author and message, with the
    /// committer git is configured with. Returns the resulting commit.
    /// `tip` itself when nothing is to replay, and `old` when `tip` is
    /// already behind it.
    fn replay(
        &self,
        old: Option<ObjectId>,
        tip: ObjectId,
    ) -> Result<(ObjectId, Vec<Replayed>), SyncError> {
        let Some(head) = old else {
            return Ok((tip, Vec::new()));
        };
        let git = self.git();
        let tip_id = self.gix_id(tip)?;
        let walk = git
            .rev_walk([self.gix_id(head)?])
            .with_hidden([tip_id])
            .all()
            .map_err(|source| self.unreadable(source))?;
        let mut local: Vec<_> = walk
            .map(|info| info.map_err(|source| self.unreadable(source)))
            .collect::<Result<_, _>>()?;
        local.reverse();
        let Some(oldest) = local.first() else {
            return Ok((tip, Vec::new()));
        };
        if oldest.parent_ids.contains(&tip_id) {
            return Ok((head, Vec::new()));
        }
        let committer = git
            .committer()
            .ok_or(WriteError::NoIdentity)?
            .map_err(|source| self.unreadable(source))?
            .to_owned()
            .map_err(|source| self.unreadable(source))?;
        let mut base = tip_id;
        let mut base_files = self.files(self.tree_of(tip_id)?)?;
        let mut replayed = Vec::new();
        for info in &local {
            let commit = git
                .find_commit(info.id)
                .map_err(|source| self.unreadable(source))?;
            let parent = info.parent_ids.first().copied();
            let before = self.files_of(parent.map(|id| model_id(&id)))?;
            let after = self.files(self.tree_of(info.id)?)?;
            let mut editor = git
                .edit_tree(self.tree_of(base)?)
                .map_err(|source| self.unreadable(source))?;
            let mut changed = false;
            for path in before.keys().chain(after.keys()) {
                let (was, now) = (before.get(path), after.get(path));
                if was == now || base_files.get(path) == now {
                    continue;
                }
                if base_files.get(path) != was {
                    return Err(SyncError::Conflict {
                        path: path.to_string(),
                    });
                }
                match now {
                    Some((id, kind)) => editor.upsert(path.as_bstr(), *kind, *id),
                    None => editor.remove(path.as_bstr()),
                }
                .map_err(|source| self.unreadable(source))?;
                match now {
                    Some(entry) => base_files.insert(path.clone(), *entry),
                    None => base_files.remove(path),
                };
                changed = true;
            }
            let from = model_id(&info.id);
            if !changed {
                replayed.push(Replayed { from, to: None });
                continue;
            }
            let tree = editor
                .write()
                .map_err(|source| self.unreadable(source))?
                .detach();
            let author = commit
                .author()
                .map_err(|source| self.unreadable(source))?
                .to_owned()
                .map_err(|source| self.unreadable(source))?;
            let message = commit
                .message_raw()
                .map_err(|source| self.unreadable(source))?
                .to_owned();
            let new = git
                .write_object(gix::objs::Commit {
                    tree,
                    parents: [base].into(),
                    author,
                    committer: committer.clone(),
                    encoding: None,
                    message,
                    extra_headers: Vec::new(),
                })
                .map_err(|source| self.unreadable(source))?
                .detach();
            replayed.push(Replayed {
                from,
                to: Some(model_id(&new)),
            });
            base = new;
        }
        Ok((model_id(&base), replayed))
    }

    /// The files of `commit`'s tree, or none for `None`.
    fn files_of(&self, commit: Option<ObjectId>) -> Result<Files, SyncError> {
        let Some(commit) = commit else {
            return Ok(Files::new());
        };
        self.files(self.tree_of(self.gix_id(commit)?)?)
    }

    /// The tree of the commit `id`.
    fn tree_of(&self, id: gix::ObjectId) -> Result<gix::ObjectId, SyncError> {
        let commit = self
            .git()
            .find_commit(id)
            .map_err(|source| self.unreadable(source))?;
        let tree = commit.tree_id().map_err(|source| self.unreadable(source))?;
        Ok(tree.detach())
    }

    /// Every file beneath the tree `root`, by path.
    fn files(&self, root: gix::ObjectId) -> Result<Files, SyncError> {
        let git = self.git();
        let mut files = Files::new();
        let mut pending = vec![(BString::default(), root)];
        while let Some((prefix, tree)) = pending.pop() {
            let tree = git
                .find_tree(tree)
                .map_err(|source| self.unreadable(source))?;
            let decoded = tree.decode().map_err(|source| self.unreadable(source))?;
            for entry in decoded.entries {
                let mut path = prefix.clone();
                path.extend_from_slice(entry.filename);
                let kind = entry.mode.kind();
                if kind == EntryKind::Tree {
                    path.push(b'/');
                    pending.push((path, entry.oid.to_owned()));
                } else {
                    files.insert(path, (entry.oid.to_owned(), kind));
                }
            }
        }
        Ok(files)
    }

    /// The paths that differ between `old` and `new`, each with the blob to
    /// write there, or `None` to remove the file. A file whose working-tree
    /// bytes are neither its old blob nor its new one is edited, and stops
    /// the sync, as does a file that is, or lies beneath a directory that
    /// is, a symlink on disk, which a write would follow; a path that is not
    /// a plain file in `new` is removed.
    fn changes(&self, old: &Files, new: &Files) -> Result<Vec<Change>, SyncError> {
        let mut changes = Vec::new();
        let paths: std::collections::BTreeSet<_> = old.keys().chain(new.keys()).collect();
        for path in paths {
            let (was, now) = (old.get(path), new.get(path));
            if was == now {
                continue;
            }
            let blob = |entry: Option<&(gix::ObjectId, EntryKind)>| match entry {
                Some((id, EntryKind::Blob | EntryKind::BlobExecutable)) => Some(model_id(id)),
                _ => None,
            };
            let (was, now) = (blob(was), blob(now));
            let path = path.to_string();
            // A prefix that is still a file here, which the remote turned
            // into a directory, holds no symlink; check_out removes it first.
            let symlink = match self.symlink_on_way(&path) {
                Err(GitError::File { source, .. })
                    if source.kind() == io::ErrorKind::NotADirectory =>
                {
                    None
                }
                other => other?,
            };
            if let Some(symlink) = symlink {
                return Err(SyncError::Symlink { path, symlink });
            }
            let held = self.working_held(&path)?;
            if held != was && held != now {
                return Err(SyncError::Edited { path });
            }
            changes.push(Change { path, was, now });
        }
        Ok(changes)
    }

    /// Moves HEAD from `old` to `new` and makes the working tree and index
    /// follow: each changed file is written or removed, then HEAD moves,
    /// then the index is written, as a put finishes. Returns each file
    /// edited since `changes` compared it, which it leaves as edited.
    fn check_out(
        &self,
        lock: &Lock,
        old: Option<ObjectId>,
        new: ObjectId,
        changes: Vec<Change>,
    ) -> Result<Vec<Fields>, SyncError> {
        let (index_lock, index) = self.lock_index()?;
        let mut staged = Staged {
            commit: new,
            parent: old,
            index,
            index_lock,
        };
        let mut kept = Vec::new();
        for change in changes {
            let held = self.working_held(&change.path)?;
            if held != change.was && held != change.now {
                kept.push(fields_at(&change.path));
            } else if let Some(blob) = change.now {
                let bytes = self
                    .read_blob(blob)?
                    .ok_or_else(|| self.missing(blob, "the blob is over the size limit"))?;
                self.write_file(lock, &change.path, &bytes)?;
            } else {
                self.remove_file(lock, &change.path)?;
            }
            match change.now {
                Some(blob) => stage_entry(&mut staged.index, &change.path, self.gix_id(blob)?),
                None => unstage_entry(&mut staged.index, &change.path),
            }
        }
        self.finish(lock, staged, "board sync")?;
        Ok(kept)
    }

    fn unreadable(&self, source: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> SyncError {
        GitError::Unreadable {
            repository: self.path().to_path_buf(),
            source: source.into(),
        }
        .into()
    }
}

/// A working-tree file to write or remove: its blob in the old commit and
/// in the new one, where `None` is no file.
struct Change {
    path: String,
    was: Option<ObjectId>,
    now: Option<ObjectId>,
}

/// Fields naming the logical path of the file at `path`.
fn fields_at(path: &str) -> Fields {
    LogicalPath::of_physical(path).map_or_else(|_| Fields::default(), |path| Fields::at(&path))
}

/// `duration` in seconds for a message, to a tenth: `1 second`,
/// `0.4 seconds`, `60 seconds`.
fn seconds(duration: Duration) -> String {
    let tenths = (duration.as_millis() + 50) / 100;
    let text = if tenths.is_multiple_of(10) {
        (tenths / 10).to_string()
    } else {
        format!("{}.{}", tenths / 10, tenths % 10)
    };
    let unit = if text == "1" { "second" } else { "seconds" };
    format!("{text} {unit}")
}

/// A sync that stopped. The local commits stay where they are.
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("{} follows no remote branch", branch.as_deref().unwrap_or("HEAD is detached, so it"))]
    NoRemote { branch: Option<String> },
    #[error("git could not be started")]
    NotStarted {
        #[source]
        source: io::Error,
    },
    #[error("git could not {operation} {remote} ({status})")]
    Failed {
        operation: &'static str,
        remote: String,
        status: String,
        stderr: Vec<u8>,
    },
    #[error(
        "git did not {operation} {remote} in the {} the sync had left of its {}, so Board stopped it",
        seconds(*left),
        seconds(*deadline)
    )]
    TimedOut {
        operation: &'static str,
        remote: String,
        /// What was left of the deadline when this git run started.
        left: Duration,
        deadline: Duration,
    },
    #[error("{path} changed on the remote and in a commit here that is not pushed")]
    Conflict { path: String },
    #[error("{path} is edited here and changed on the remote")]
    Edited { path: String },
    #[error("{path} cannot be written: {symlink} is a symlink")]
    Symlink { path: String, symlink: String },
    #[error("another sync of this clone is running; {} is held", lock.display())]
    Busy { lock: std::path::PathBuf },
    #[error("the remote refused the push {attempts} times, the last time with {reason}")]
    GaveUp { attempts: u32, reason: String },
    #[error(transparent)]
    Write(#[from] WriteError),
    #[error(transparent)]
    Git(#[from] GitError),
}

impl SyncError {
    pub fn code(&self) -> Code {
        match self {
            Self::NoRemote { .. } => Code::SyncNoRemote,
            Self::NotStarted { .. } | Self::Failed { .. } => Code::SyncFailed,
            Self::Conflict { .. } => Code::SyncConflict,
            Self::Edited { .. } => Code::SyncEdited,
            Self::Symlink { .. } => Code::PutSymlink,
            Self::Busy { .. } => Code::SyncBusy,
            Self::GaveUp { .. } => Code::SyncGaveUp,
            Self::TimedOut { .. } => Code::SyncTimeout,
            Self::Write(error) => error.code(),
            Self::Git(error) => error.code(),
        }
    }

    /// The logical path of the file at fault, when it has one.
    pub fn fields(&self) -> Fields {
        match self {
            Self::Conflict { path } | Self::Edited { path } | Self::Symlink { path, .. } => {
                fields_at(path)
            }
            Self::Write(error) => error.fields(),
            _ => Fields::default(),
        }
    }

    /// The end of git's stderr, for a run git refused.
    pub fn stderr(&self) -> &[u8] {
        match self {
            Self::Failed { stderr, .. } => stderr,
            _ => &[],
        }
    }
}
