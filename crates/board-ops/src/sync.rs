//! `board sync`: bringing a board up to date with the remote branch it
//! follows and pushing what it committed (docs/architecture.md, Writes).

use std::time::Duration;

use board_model::{Code, Refusal};
pub use board_tree::Synced;
use board_tree::{Deadline, LOCK_WAIT};

use crate::board::Board;

impl Board {
    /// Fetches the branch HEAD follows, replays the commits only this
    /// board has onto it, pushes the result when `push`, and moves HEAD,
    /// the index and the working tree to it, under the board lock so no
    /// put runs meanwhile. It waits for the lock up to [`LOCK_WAIT`], and
    /// git still fetching or pushing `deadline` after the sync began is
    /// stopped.
    pub fn sync(&self, push: bool, deadline: Duration) -> Result<Synced, SyncError> {
        let deadline = Deadline::after(deadline);
        let repository = self.repository();
        let lock = (self.repositories())
            .lock(deadline.left().min(LOCK_WAIT))
            .map_err(board_tree::SyncError::from)?;
        Ok(repository.sync(&lock, push, deadline)?)
    }
}

/// A sync that stopped before it moved HEAD.
#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct SyncError(#[from] board_tree::SyncError);

impl SyncError {
    pub fn code(&self) -> Code {
        self.0.code()
    }

    /// This failure as Board reports it, with the end of git's stderr
    /// when git refused a fetch or push.
    pub fn refusal(&self) -> Refusal {
        let mut refusal = Refusal::new(self.code(), self, &self.0.fields());
        let stderr = self.0.stderr();
        if !stderr.is_empty() {
            let text = String::from_utf8_lossy(stderr);
            refusal.fields.insert("stderr".to_owned(), text.into());
            refusal.stderr = stderr.to_vec();
        }
        refusal
    }
}

#[cfg(test)]
mod tests;
