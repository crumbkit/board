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
use super::{Lock, Staged, WriteError};
use crate::git::{GitError, Repository, model_id};
use crate::transport::{self, Pushed, Upstream};

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

    /// Fetches the upstream, replays the commits only this clone has onto
    /// it, and, with `push`, pushes the result, fetching again when the
    /// remote moved meanwhile, up to [`ATTEMPTS`] times. Then HEAD, the
    /// index and the working tree move to the result. A file the remote
    /// changed that is edited here stops the sync before it pushes or
    /// writes; one edited after that, such as during the push, is kept as
    /// edited and listed in `kept`. `lock` is the board lock. Each fetch
    /// and push that runs past `deadline` is stopped.
    pub fn sync(&self, lock: &Lock, push: bool, deadline: Duration) -> Result<Synced, SyncError> {
        let upstream = self.upstream()?;
        let old = self.head()?;
        let old_files = self.files_of(old)?;
        let mut reason = String::new();
        let (attempts, fetched, new, replayed, pushed, changes) = 'synced: {
            for attempts in 1..=ATTEMPTS {
                transport::fetch(self.path(), &upstream, deadline)?;
                let fetched = self.tracking(&upstream)?;
                let (new, replayed) = self.replay(old, fetched)?;
                let changes = self.changes(&old_files, &self.files_of(Some(new))?)?;
                if !push || new == fetched {
                    break 'synced (attempts, fetched, new, replayed, false, changes);
                }
                match transport::push(self.path(), &upstream, &new.to_string(), deadline)? {
                    Pushed::Done => {
                        break 'synced (attempts, fetched, new, replayed, true, changes);
                    }
                    Pushed::Rejected(why) => reason = why,
                }
            }
            return Err(SyncError::GaveUp {
                attempts: ATTEMPTS,
                reason,
            });
        };
        let kept = if old == Some(new) {
            Vec::new()
        } else {
            self.check_out(lock, old, new, changes)?
        };
        Ok(Synced {
            upstream,
            fetched,
            old,
            new,
            replayed,
            pushed,
            attempts,
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
    let logical = path.strip_suffix(".json").unwrap_or(path);
    format!("/{logical}")
        .parse::<LogicalPath>()
        .map(|path| Fields::at(&path))
        .unwrap_or_default()
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
    #[error("git did not {operation} {remote} within {} seconds, so Board stopped it", deadline.as_secs())]
    TimedOut {
        operation: &'static str,
        remote: String,
        deadline: Duration,
    },
    #[error("{path} changed on the remote and in a commit here that is not pushed")]
    Conflict { path: String },
    #[error("{path} is edited here and changed on the remote")]
    Edited { path: String },
    #[error("{path} cannot be written: {symlink} is a symlink")]
    Symlink { path: String, symlink: String },
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
