//! `board mount update`: advancing a mount to the commit its `follow` ref
//! names (docs/architecture.md, Writes).

use board_model::{BINARY_NAME, Code, LogicalPath, ObjectId, Refusal};
use board_tree::{LOCK_WAIT, Lock, Pin, PinError, Target};

use crate::board::Board;
use crate::put::{Expect, Failure, Prepared, PutError, PutReport, Schemas};

/// What `mount update` found and whether it wrote.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MountUpdateReport {
    pub path: LogicalPath,
    pub old: ObjectId,
    pub new: ObjectId,
    pub wrote: bool,
}

impl Board {
    /// Rewrites the mount pointer at `path`, which is not followed, to the
    /// commit its `follow` ref names now, through put with the pointer's
    /// blob as the expect. The pointer is read under the lock the write
    /// holds. A mount already there is left as it is.
    pub fn mount_update(&self, path: &LogicalPath) -> Result<MountUpdateReport, MountError> {
        let lock = self
            .repository()
            .lock(LOCK_WAIT)
            .map_err(|error| MountError::Put(error.into()))?;
        let Pin {
            target,
            blob,
            old,
            new,
            canonical,
            ..
        } = self.repositories().pin(path)?;
        let mut report = MountUpdateReport {
            path: path.clone(),
            old,
            new,
            wrote: false,
        };
        if new == old {
            return Ok(report);
        }
        let message = format!("{BINARY_NAME} mount update {path}");
        self.repin(lock, path, target, blob, &canonical, &message)?;
        report.wrote = true;
        Ok(report)
    }

    /// Rewrites the mount pointer at `path`, found at `target` holding
    /// `blob` under `lock`, to `canonical`, through put with that blob as
    /// the expect.
    pub(crate) fn repin(
        &self,
        lock: Lock,
        path: &LogicalPath,
        target: Target,
        blob: ObjectId,
        canonical: &[u8],
        message: &str,
    ) -> Result<PutReport, PutError> {
        let admitted = self
            .check_record(&mut Schemas::default(), canonical)
            .map_err(|refused| PutError::Record {
                path: path.clone(),
                refused,
            })?;
        let prepared = Prepared {
            path: path.clone(),
            expect: Expect::Blob(blob),
            canonical: canonical.to_vec(),
            target,
            admitted,
        };
        self.write_all(lock, &[prepared], &[], message)
            .map_err(|failure| match failure {
                Failure::Stale(mut stale) => stale.remove(0).1,
                Failure::Error(error) => error,
            })
    }
}

/// A mount update that wrote nothing, or, for a put's `head_moved`, wrote
/// but did not commit.
#[derive(Debug, thiserror::Error)]
pub enum MountError {
    #[error(transparent)]
    Pin(#[from] PinError),
    #[error(transparent)]
    Put(#[from] PutError),
}

impl MountError {
    pub fn code(&self) -> Code {
        match self {
            Self::Pin(error) => error.code(),
            Self::Put(error) => error.code(),
        }
    }

    pub fn refusal(&self) -> Refusal {
        let fields = match self {
            Self::Pin(error) => error.fields(),
            Self::Put(error) => error.fields(),
        };
        Refusal::new(self.code(), self, &fields)
    }
}

#[cfg(test)]
mod tests;
