//! `board rm`: removing the file a logical path names
//! (docs/architecture/writes/removing.md).

use std::time::Duration;

use board_model::{BINARY_NAME, LogicalPath};
use board_tree::{Lock, Resolver, WriteError};

use super::{Expect, Failure, PutError, PutReport, Removal, Schemas};
use crate::board::Board;
use crate::effects::Change;

impl Board {
    /// Takes the lock, waiting up to `wait`, and removes the file `path`
    /// names, only if it is what `expect` says, with one commit when
    /// `commit_on_put` is set.
    pub fn remove_waiting(
        &self,
        path: &LogicalPath,
        expect: Expect,
        wait: Duration,
    ) -> Result<PutReport, PutError> {
        let removed = self.remove_file(path, expect, wait);
        self.holding(removed)
    }

    /// [`Board::remove_waiting`], leaving any pending effect it gathers for
    /// [`Board::holding`].
    fn remove_file(
        &self,
        path: &LogicalPath,
        expect: Expect,
        wait: Duration,
    ) -> Result<PutReport, PutError> {
        let lock = self.repository().lock(wait)?;
        let mut view = self.repositories().snapshot().map_err(WriteError::from)?;
        let schemas = &mut Schemas::default();
        let removal = self.prepare_removal(&lock, &mut view, schemas, path, expect)?;
        let message = format!("{BINARY_NAME} rm {path}");
        self.write_all(lock, &[], &[removal], &message)
            .map_err(Failure::one)
    }

    /// Finds the file `path` names in `view`, under the lock that the
    /// removal will hold, compares it with `expect`, and checks the
    /// removal against the effects rules. `Expect::Absent` never holds,
    /// since the file is there.
    pub(crate) fn prepare_removal(
        &self,
        _lock: &Lock,
        view: &mut Resolver<'_, '_>,
        schemas: &mut Schemas<'_>,
        path: &LogicalPath,
        expect: Expect,
    ) -> Result<Removal, PutError> {
        let (target, removes) = view.removal_target(path)?;
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
        self.gate(&mut schemas.effects, &target, Change::Remove(removes))?;
        Ok(Removal {
            path: path.clone(),
            file: target.path,
            blob,
        })
    }
}
