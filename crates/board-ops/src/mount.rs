//! `board mount update`: advancing a mount to the commit its `follow` ref
//! names (docs/architecture.md, Writes).

use board_model::{BINARY_NAME, Code, LogicalPath, ObjectId};
use board_tree::{Pin, PinError};

use crate::board::Board;
use crate::put::{Expect, Failure, Prepared, PutError};

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
    /// blob as the expect. A mount already there is left as it is.
    pub fn mount_update(&self, path: &LogicalPath) -> Result<MountUpdateReport, MountError> {
        let Pin {
            target,
            blob,
            old,
            new,
            canonical,
        } = self.repository().pin(path)?;
        let mut report = MountUpdateReport {
            path: path.clone(),
            old,
            new,
            wrote: false,
        };
        if new == old {
            return Ok(report);
        }
        self.check_record(&canonical).map_err(|refused| {
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
        };
        self.write_all(&[prepared], &format!("{BINARY_NAME} mount update {path}"))
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
}

#[cfg(test)]
mod tests;
