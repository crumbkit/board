//! `board mount update`: advancing a mount to the commit its `follow` ref
//! names (docs/architecture.md, Writes).

use board_model::{BINARY_NAME, Code, Fields, LogicalPath, ObjectId};
use board_tree::{LOCK_WAIT, Pin, PinError};

use crate::board::Board;
use crate::put::{Expect, Failure, Prepared, PutError, Schemas};

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
        let admitted = self
            .check_record(&mut Schemas::default(), &canonical)
            .map_err(|refused| {
                MountError::Put(PutError::Record {
                    path: path.clone(),
                    refused,
                })
            })?;
        let prepared = Prepared {
            path: path.clone(),
            expect: Expect::Blob(blob),
            canonical,
            target,
            admitted,
        };
        let message = format!("{BINARY_NAME} mount update {path}");
        self.write_all(lock, &[prepared], &message)
            .map_err(|failure| {
                MountError::Put(match failure {
                    Failure::Stale(mut stale) => stale.remove(0).1,
                    Failure::Error(error) => error,
                })
            })?;
        report.wrote = true;
        Ok(report)
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

    pub fn fields(&self) -> Fields {
        match self {
            Self::Pin(error) => error.fields(),
            Self::Put(error) => error.fields(),
        }
    }
}

#[cfg(test)]
mod tests;
