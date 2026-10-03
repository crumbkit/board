//! `board mount update`: advancing a mount to the commit its `follow` ref
//! names (docs/architecture.md, Writes).

use board_model::{
    BINARY_NAME, Code, LogicalPath, Object, ObjectId, Pointer, canonical_json, parse_json,
};
use serde_json::Value;

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
        let repository = self.repository();
        let target = repository
            .pointer_target(path)
            .map_err(|error| MountError::Put(error.into()))?;
        let Object::Blob(blob) = target.location.object else {
            return Err(MountError::NotAMount { path: path.clone() });
        };
        let bytes = repository
            .read_working_file(&target.path)
            .map_err(|error| MountError::Put(error.into()))?
            .ok_or_else(|| MountError::NotAMount { path: path.clone() })?;
        let mount = match board_model::pointer(&bytes) {
            Ok(Some(Pointer::Mount(mount))) => mount,
            Ok(_) => return Err(MountError::NotAMount { path: path.clone() }),
            Err(error) => {
                return Err(MountError::Mount {
                    path: path.clone(),
                    code: error.code(),
                });
            }
        };
        let new = repository
            .followed(&mount)
            .map_err(|code| MountError::Mount {
                path: path.clone(),
                code,
            })?;
        let mut report = MountUpdateReport {
            path: path.clone(),
            old: mount.commit,
            new,
            wrote: false,
        };
        if new == mount.commit {
            return Ok(report);
        }
        let mut value: Value = parse_json(&bytes).map_err(|error| MountError::Mount {
            path: path.clone(),
            code: error.code(),
        })?;
        value["commit"] = Value::String(new.to_string());
        let canonical = canonical_json(&value);
        self.check_record(&canonical).map_err(|code| {
            MountError::Put(PutError::Record {
                path: path.clone(),
                code,
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
    #[error("{path} is not a mount pointer")]
    NotAMount { path: LogicalPath },
    #[error("the mount at {path} cannot be updated: {code}")]
    Mount { path: LogicalPath, code: Code },
    #[error(transparent)]
    Put(#[from] PutError),
}

impl MountError {
    pub fn code(&self) -> Code {
        match self {
            Self::NotAMount { .. } => Code::MountInvalid,
            Self::Mount { code, .. } => *code,
            Self::Put(error) => error.code(),
        }
    }
}

#[cfg(test)]
mod tests;
