//! `board rm`: removing the file a logical path names, or every file
//! beneath it (docs/architecture/writes/removing.md).

use std::time::Duration;

use board_model::{BINARY_NAME, Code, LogicalPath};
use board_tree::{Lock, Removes, Resolver, Target, WriteError};

use super::{Expect, Failure, PutError, PutReport, Removal};
use crate::board::Board;
use crate::effects::Change;

/// What `board rm` removes at a path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Removing {
    /// The one file the path names, only if it is what the expect says.
    File(Expect),
    /// Every file beneath the directory the path names, as `rm -r` does.
    Beneath,
}

/// What a removal of a path that names nothing does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unmatched {
    /// Refuses it with `board::path::not_found`.
    Refuse,
    /// Removes nothing and reports the path absent.
    Ignore,
}

impl Board {
    /// Takes the lock, waiting up to `wait`, and removes what `removing`
    /// says at `path`, with one commit when `commit_on_put` is set.
    pub fn remove_waiting(
        &self,
        path: &LogicalPath,
        removing: Removing,
        unmatched: Unmatched,
        wait: Duration,
    ) -> Result<PutReport, PutError> {
        let lock = self.repository().lock(wait)?;
        let mut view = self.repositories().snapshot().map_err(WriteError::from)?;
        let (removals, message) = match removing {
            Removing::File(expect) => (
                self.prepare_removal(&lock, &mut view, path, expect)
                    .map(|removal| vec![removal]),
                format!("{BINARY_NAME} rm {path}"),
            ),
            Removing::Beneath => (
                self.prepare_removals(&mut view, path),
                format!("{BINARY_NAME} rm -r {path}"),
            ),
        };
        let removals = match removals {
            Err(error) if unmatched.ignores(&error) => return Ok(PutReport::absent(path.clone())),
            removals => removals?,
        };
        self.write_all(lock, &[], &removals, &message)
            .map_err(Failure::one)
    }

    /// Finds the file `path` names in `view`, under the lock that the
    /// removal will hold, compares it with `expect`, and checks
    /// that only the owner may remove it. `Expect::Absent` never holds,
    /// since the file is there.
    pub(crate) fn prepare_removal(
        &self,
        _lock: &Lock,
        view: &mut Resolver<'_, '_>,
        path: &LogicalPath,
        expect: Expect,
    ) -> Result<Removal, PutError> {
        let (target, removes) = view.removal_target(path)?;
        self.checked_removal(path, target, removes, expect)
    }

    /// Each file beneath the directory `path` names in `view`, checked as
    /// one removal is.
    fn prepare_removals(
        &self,
        view: &mut Resolver<'_, '_>,
        path: &LogicalPath,
    ) -> Result<Vec<Removal>, PutError> {
        (view.removal_targets(path)?.into_iter())
            .map(|(logical, target, removes)| {
                self.checked_removal(&logical, target, removes, Expect::Any)
            })
            .collect()
    }

    /// The removal of `target`, which `path` names, if it holds what
    /// `expect` says and the gate lets it go.
    fn checked_removal(
        &self,
        path: &LogicalPath,
        target: Target,
        removes: Removes,
        expect: Expect,
    ) -> Result<Removal, PutError> {
        let blob = (self.repository().working_blob(&target.path)).map_err(WriteError::from)?;
        let holds = match expect {
            Expect::Any => true,
            Expect::Absent => false,
            Expect::Blob(expected) => expected == blob,
        };
        if !holds {
            return Err(PutError::Stale {
                path: path.clone(),
                found: Some(blob),
            });
        }
        self.gate(&target, Change::Remove(removes))?;
        Ok(Removal {
            path: path.clone(),
            file: target.path,
            blob,
        })
    }
}

impl Unmatched {
    /// Whether `error` refuses a path that names nothing, and that is
    /// ignored.
    pub(crate) fn ignores(self, error: &PutError) -> bool {
        self == Self::Ignore && error.code() == Code::PathNotFound
    }
}
