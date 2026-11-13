//! Finishing a put's commit once its files are written
//! (docs/architecture.md, Writes).

use board_model::ObjectId;
use board_tree::{Lock, Staged, WriteError};

use super::{Failure, PutError, Written};
use crate::board::Board;

impl Board {
    /// Moves HEAD to `staged`, which is staged on a HEAD the put compared
    /// its `--expect` against as `seen`, as
    /// [`board_tree::Repository::finish_commit_retrying`] does. A HEAD that
    /// moved for good leaves `writes` written and uncommitted.
    pub(super) fn finish_put(
        &self,
        lock: &Lock,
        staged: Option<Staged>,
        seen: Option<ObjectId>,
        files: &[(&str, Option<&[u8]>)],
        message: &str,
        writes: Vec<Written>,
    ) -> Result<Option<ObjectId>, Failure> {
        let Some(staged) = staged else {
            return Ok(None);
        };
        let between = self.between.run();
        match (self.repository())
            .finish_commit_retrying(lock, staged, seen, files, message, between)
        {
            Err(WriteError::HeadMoved) => Err(Failure::Error(PutError::HeadMoved { writes })),
            done => done.map_err(|error| Failure::Error(error.into())),
        }
    }
}
