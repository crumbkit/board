//! What a put through a mount needs to know of the mounted repository
//! (docs/architecture/writes.md): whether its HEAD may be written on, and
//! what a record file holds at the pin, at HEAD and in the working tree.

use std::collections::HashSet;
use std::path::PathBuf;

use board_model::{Code, Location, LogicalPath, ObjectId, Source};

use crate::git::{GitError, Repository, model_id};
use crate::worktree::Working;
use crate::write::WriteError;

/// The most commits a put walks back from the mounted HEAD looking for the
/// pinned commit, so a huge mounted history cannot stall a put.
pub(crate) const ANCESTRY: usize = 100_000;

/// Where a path through one mount lands in the mounted board.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inside {
    /// The mounted repository's working directory.
    pub repository: PathBuf,
    /// The commit the mount is pinned to.
    pub commit: ObjectId,
    /// The path's file in the mounted tree, `/`-separated.
    pub file: String,
    /// The record's logical path in the mounted board.
    pub path: LogicalPath,
}

/// Why a path through a mount is written or refused, as the clause a
/// refusal adds after naming the mount.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Through {
    /// One mount, which a put or a batch may write through.
    #[error("")]
    Mounted,
    #[error(", and a put writes through one mount at most")]
    Nested,
    #[error(", which is this board's own repository")]
    Own,
    #[error(", whose repository has no working tree with a .board/board.json")]
    NotABoard,
    #[error(", whose board sets commit_on_put to false")]
    Uncommitted,
    #[error(", whose follow {follow} is not a local branch")]
    Remote { follow: String },
    /// `found` is `None` when HEAD is detached.
    #[error(", whose HEAD is {}, not on {needs}", found.as_ref().map_or_else(|| "detached".to_owned(), |found| format!("on {found}")))]
    Branch {
        found: Option<String>,
        needs: String,
    },
    #[error(", whose HEAD {head} does not descend from the mount's commit {commit}")]
    NotAncestor { head: ObjectId, commit: ObjectId },
    #[error(", whose {file} has changed since the mount's commit")]
    Moved { file: String },
}

impl Through {
    pub fn code(&self) -> Code {
        match self {
            Self::NotAncestor { .. } => Code::MountNotAncestor,
            Self::Moved { .. } => Code::MountStale,
            _ => Code::PutThroughMount,
        }
    }
}

impl Repository {
    /// The commit HEAD names, if a put may commit on it for a mount that
    /// follows `follow` and is pinned to `pin`: HEAD is on the local branch
    /// `follow` names and descends from `pin`.
    pub fn writable_head(
        &self,
        follow: &str,
        pin: ObjectId,
    ) -> Result<Result<ObjectId, Through>, GitError> {
        let unreadable = |source: Box<dyn std::error::Error + Send + Sync>| GitError::Unreadable {
            repository: self.path().to_path_buf(),
            source,
        };
        let needs = self
            .git()
            .try_find_reference(follow)
            .map_err(|error| unreadable(error.into()))?
            .map(|reference| reference.name().as_bstr().to_string());
        let Some(needs) = needs.filter(|needs| needs.starts_with("refs/heads/")) else {
            let follow = follow.to_owned();
            return Ok(Err(Through::Remote { follow }));
        };
        let found = self
            .git()
            .head_name()
            .map_err(|error| unreadable(error.into()))?
            .map(|name| name.as_bstr().to_string());
        let head = match self.head()? {
            Some(head) if found.as_ref() == Some(&needs) => head,
            _ => return Ok(Err(Through::Branch { found, needs })),
        };
        Ok(if self.descends(head, pin, ANCESTRY)? {
            Ok(head)
        } else {
            Err(Through::NotAncestor { head, commit: pin })
        })
    }

    /// Whether `commit` is `ancestor` or descends from it, looking at no more
    /// than `limit` commits.
    pub(crate) fn descends(
        &self,
        commit: ObjectId,
        ancestor: ObjectId,
        limit: usize,
    ) -> Result<bool, GitError> {
        let mut seen = HashSet::new();
        let mut pending = vec![commit];
        while let Some(commit) = pending.pop() {
            if commit == ancestor {
                return Ok(true);
            }
            if !seen.insert(commit) {
                continue;
            }
            if seen.len() > limit {
                return Ok(false);
            }
            let found = self
                .git()
                .find_commit(self.gix_id(commit)?)
                .map_err(|error| self.missing(commit, error))?;
            pending.extend(found.parent_ids().map(|parent| model_id(&parent)));
        }
        Ok(false)
    }

    /// The blob the `/`-separated `file` holds at `pin`, at `head` and in
    /// the working tree; `None` where it is absent.
    pub fn held(
        &self,
        pin: ObjectId,
        head: ObjectId,
        file: &str,
    ) -> Result<[Option<ObjectId>; 3], GitError> {
        let at = |commit: ObjectId| -> Result<Option<ObjectId>, GitError> {
            let tree = self
                .git()
                .find_commit(self.gix_id(commit)?)
                .and_then(|found| found.tree())
                .map_err(|error| self.missing(commit, error))?;
            let entry = tree
                .lookup_entry_by_path(file)
                .map_err(|error| self.missing(commit, error))?;
            Ok(entry
                .filter(|entry| entry.mode().is_blob())
                .map(|entry| model_id(entry.oid())))
        };
        let working = match self.read_working(file)? {
            Working::Missing => None,
            Working::Read(contents) => Some(contents.blob),
            Working::TooLarge => Some(self.working_blob(file)?),
        };
        Ok([at(pin)?, at(head)?, working])
    }
}

/// The last pointer in this board's working tree on the way to `location`:
/// the mount that `path` passes. A mount file with no logical path, such as
/// one reached through a symlink by a path longer than a logical path may
/// be, has no `display_path`; then it is `path`.
pub(crate) fn mount_passed(path: &LogicalPath, location: &Location) -> LogicalPath {
    (location.pointers.iter())
        .take_while(|pointer| pointer.board.source == Source::WorkingTree)
        .last()
        .and_then(Location::display_path)
        .unwrap_or_else(|| path.clone())
}

/// A write to `path` refused because `location`, where it leads, is in a
/// mounted board, naming the last pointer in the working tree: the mount
/// that leads out of it. Through one mount to the file `physical`, whose
/// logical path drops `suffix`, it says where that file is in the mounted
/// board, for a put to write.
pub(crate) fn through_mount(
    path: &LogicalPath,
    location: &Location,
    physical: Option<&str>,
    suffix: &str,
) -> WriteError {
    let mut mounted = location
        .pointers
        .iter()
        .filter(|pointer| pointer.board.source != Source::WorkingTree);
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
    WriteError::ThroughMount {
        path: path.clone(),
        mount: mount_passed(path, location),
        why: Box::new(why),
        inside,
    }
}
