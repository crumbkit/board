//! Writing a file of a notes repo, the refusals a write retries on, and
//! the sync and pin that land it. plugins/notes/README.md says how a
//! note's write is retried.

use std::cell::RefCell;
use std::path::Path;
use std::time::Duration;

use board_model::{Code, Fields, LogicalPath, PathError, Refusal};
use board_ops::{Conflicted, Elsewhere, Host, PluginError, PutError};
use serde_json::json;

use crate::refusal;
use crate::repo::Repo;

/// Refusals of a write because the file or the clone moved after the
/// read; the write is tried again on a new read. A put to the board's own
/// tree refused with `board::put::head_moved` wrote the file and did not
/// commit it.
pub const MOVED: [Code; 2] = [Code::PutStale, Code::PutHeadMoved];
/// The refusal of a write over an edit not committed in the clone, which
/// no new read clears.
pub const EDITED: Code = Code::MountStale;
pub const ATTEMPTS: usize = 3;
/// The sync refusals after which a write can still be made: the clone is
/// left as it was or as an earlier round of the sync made it, and either
/// way the write parks beside it.
const UNSYNCED: [Code; 2] = [Code::SyncEdited, Code::SyncBusy];

/// What landing a write needs of Board beyond what [`Host`] has on main:
/// `board -C <clone> sync` and `board mount update`, each within a
/// deadline. Each mirrors `Host::sync` on the host `Host::board` gives
/// for the clone's directory, and `Host::mount_update`, with the error's
/// refusal in place of the error.
pub trait Clones {
    /// Syncs the clone at `clone` with its remote, pushing when `push`.
    fn sync(&self, clone: &Path, push: bool, deadline: Duration) -> Result<(), Box<Refusal>>;
    /// Moves the mount at `path` to its clone's HEAD and returns that
    /// commit.
    fn mount_update(&self, path: &LogicalPath, deadline: Duration) -> Result<String, Box<Refusal>>;
    /// The conflicts the syncs made so far found: each file whose merge
    /// kept one side's content.
    fn conflicts(&self) -> Vec<Conflicted>;
}

/// The clones Board syncs and pins for a call, through its host: a sync
/// on the host `board -C <clone>` opens, and the pin as `board mount
/// update` moves it.
pub struct Through<'a> {
    host: &'a dyn Host,
    found: RefCell<Vec<Conflicted>>,
}

impl<'a> Through<'a> {
    pub fn new(host: &'a dyn Host) -> Self {
        Self {
            host,
            found: RefCell::new(Vec::new()),
        }
    }
}

impl std::fmt::Debug for Through<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Through")
    }
}

impl Clones for Through<'_> {
    fn sync(&self, clone: &Path, push: bool, deadline: Duration) -> Result<(), Box<Refusal>> {
        let board = self
            .host
            .board(&Elsewhere::Directory(clone.to_path_buf()))?;
        let synced = (board.sync_waiting(push, deadline, board_ops::LOCK_WAIT))
            .map_err(|error| Box::new(error.refusal()))?;
        self.found.borrow_mut().extend(synced.conflicts);
        Ok(())
    }

    fn mount_update(&self, path: &LogicalPath, deadline: Duration) -> Result<String, Box<Refusal>> {
        (self.host.mount_update(path, deadline))
            .map(|report| report.new.to_string())
            .map_err(|error| Box::new(error.refusal()))
    }

    fn conflicts(&self) -> Vec<Conflicted> {
        self.found.borrow().clone()
    }
}

/// What a write reaches: the host, the clones, the plugin whose codes it
/// gives, and the deadline of each sync and pin.
#[derive(Clone, Copy)]
pub struct Writer<'a> {
    pub host: &'a dyn Host,
    pub clones: &'a dyn Clones,
    pub plugin: &'a str,
    pub deadline: Duration,
}

impl std::fmt::Debug for Writer<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Writer")
            .field("plugin", &self.plugin)
            .field("deadline", &self.deadline)
            .finish_non_exhaustive()
    }
}

/// The code Board refused a write with. A batch reports its one line's
/// refusal among its refusals.
pub fn refused(error: &PutError) -> Code {
    match error {
        PutError::Batch { refusals, .. } if refusals.len() == 1 => refusals[0].code,
        other => other.code(),
    }
}

impl Writer<'_> {
    /// The paths, logical in `repo`, of the conflict records that name a
    /// file of `wrote` (paths in the clone), once each, in path order, from
    /// every sync made through this writer's clones. Such a record says a
    /// merge kept one side's content of a file the call wrote, which may
    /// not be the call's.
    pub fn conflicts_at(&self, repo: &Repo, wrote: &[&str]) -> Vec<String> {
        let mut found: Vec<String> = (self.clones.conflicts().iter())
            .filter(|conflict| wrote.contains(&conflict.path.as_str()))
            .map(|conflict| repo.logical(&conflict.record))
            .collect();
        found.sort();
        found.dedup();
        found
    }
}

impl Repo {
    /// Syncs the clone with its remote: none, or the code of a refusal
    /// that left HEAD where it was. Any other refusal is
    /// `<plugin>::board::refused`. The board's own tree is never synced,
    /// so a caller does not sync it.
    pub fn sync(&self, writer: Writer<'_>, push: bool) -> Result<Option<Code>, PluginError> {
        let Some(clone) = &self.clone else {
            return Ok(None);
        };
        match (writer.clones).sync(Path::new(clone), push, writer.deadline) {
            Ok(()) => Ok(None),
            Err(refused) => match UNSYNCED
                .iter()
                .find(|code| code.as_str() == refused.code.as_str())
            {
                Some(code) => Ok(Some(*code)),
                None => Err(PluginError::board_refused(writer.plugin, "sync", &refused)),
            },
        }
    }

    /// Moves the mount's pin to the clone's HEAD and returns that commit;
    /// none for the board's own tree.
    pub fn pin(&self, writer: Writer<'_>) -> Result<Option<String>, PluginError> {
        if self.clone.is_none() {
            return Ok(None);
        }
        let refused =
            |refused: &Refusal| PluginError::board_refused(writer.plugin, "mount update", refused);
        let path: LogicalPath = (self.root.parse()).map_err(|error: PathError| {
            refused(&Refusal::new(error.code(), &error, &Fields::default()))
        })?;
        (writer.clones.mount_update(&path, writer.deadline))
            .map(Some)
            .map_err(|error| refused(&error))
    }
}

/// Writes `text` to a new file at `<stem>.md`, or at `<stem>-2.md` and on
/// when one is there: its path and commit, none when the write is left
/// uncommitted. It tries `names` names, then refuses with the code `busy`.
pub fn create(
    writer: Writer<'_>,
    repo: &Repo,
    stem: &str,
    text: &str,
    names: usize,
    busy: &str,
) -> Result<(String, Option<String>), PluginError> {
    let Writer { host, plugin, .. } = writer;
    let (mut number, mut moved) = (1, 0);
    while number <= names {
        let path = if number == 1 {
            format!("{stem}.md")
        } else {
            format!("{stem}-{number}.md")
        };
        repo.pin(writer)?;
        let error = match repo.write(host, &path, text, None) {
            Ok(commit) => return Ok((path, commit)),
            Err(error) => error,
        };
        match refused(&error) {
            // The put whose HEAD moved wrote this file, so it is this file.
            Code::PutStale if moved > 0 => return Ok((path, None)),
            Code::PutStale => number += 1,
            Code::PutHeadMoved => {
                moved += 1;
                if repo.clone.is_none() || moved == ATTEMPTS {
                    // Board wrote the file before HEAD moved, so it is in
                    // the working tree, uncommitted.
                    return Ok((path, None));
                }
            }
            _ => return Err(PluginError::board_refused(plugin, "put", &error.refusal())),
        }
    }
    let why = format!(
        "{}.md and the {} names after it are taken; nothing was written; call again",
        repo.logical(stem),
        names.saturating_sub(1)
    );
    let path = repo.logical(&format!("{stem}.md"));
    Err(refusal(busy.to_owned(), why, [("path", json!(path))]))
}

/// Syncs the clone, pushing: [`synced`] with `push`.
pub fn push(writer: Writer<'_>, repo: &Repo) -> Option<String> {
    synced(writer, repo, true)
}

/// Syncs the clone, pushing only when `push`: none, or the code of the
/// refusal. The code is `<plugin>::repo::not_synced` for the board's own
/// tree, never synced. A failed push after a write is not retried, since
/// the file is committed.
pub fn synced(writer: Writer<'_>, repo: &Repo, push: bool) -> Option<String> {
    if repo.clone.is_none() {
        return Some(format!("{}::repo::not_synced", writer.plugin));
    }
    match repo.sync(writer, push) {
        Ok(code) => code.map(|code| code.as_str().to_owned()),
        Err(error) => Some(
            (error.fields.get("code").and_then(|code| code.as_str()))
                .map_or(error.code, str::to_owned),
        ),
    }
}

/// Pushes a write, then moves the mount's pin: [`landed_with`] with
/// `push`.
pub fn landed(
    writer: Writer<'_>,
    repo: &Repo,
    commit: Option<String>,
) -> Result<(Option<String>, Option<String>), PluginError> {
    landed_with(writer, repo, commit, true)
}

/// Pushes a write when `push`, then moves the mount's pin to the clone's
/// HEAD: the commit holding the write, and [`synced`]'s code, or
/// `<plugin>::repo::not_pushed` for a clone left unpushed. A put through a
/// mount pins the commit it made, and the push may rebase that commit onto
/// the remote's, so the pin moves after the push and the commit returned
/// is the clone's HEAD. `commit` is none when the put left the write
/// uncommitted.
pub fn landed_with(
    writer: Writer<'_>,
    repo: &Repo,
    commit: Option<String>,
    push: bool,
) -> Result<(Option<String>, Option<String>), PluginError> {
    let unpushed = if push || repo.clone.is_none() {
        synced(writer, repo, true)
    } else {
        Some(format!("{}::repo::not_pushed", writer.plugin))
    };
    let Some(commit) = commit else {
        return Ok((None, unpushed));
    };
    let pinned = repo.pin(writer)?;
    Ok((Some(pinned.unwrap_or(commit)), unpushed))
}

#[cfg(test)]
mod tests;
