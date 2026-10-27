//! Where a removal lands (docs/architecture/writes/removing.md).

use board_model::{Location, LogicalPath, Object, Source};

use super::{Target, WriteError};
use crate::resolve::Resolver;
use crate::through::mount_passed;

/// What a removal removes: a file, or a link or mount record itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Removes {
    File,
    Pointer,
}

impl Resolver<'_, '_> {
    /// The file `path` names, for removing, and what it is.
    /// A pointer at the end is not followed, as `git rm` removes a symlink
    /// and not what it leads to. A path that names nothing, a directory, or
    /// a file in a mounted board is refused.
    pub fn removal_target(&mut self, path: &LogicalPath) -> Result<(Target, Removes), WriteError> {
        let location = self.removable(path)?;
        let Object::Blob(_) = location.object else {
            return Err(WriteError::Directory { path: path.clone() });
        };
        self.repository().refuse_symlink(path, &location.path)?;
        let removes = if self.resolve(path).ok().as_ref() == Some(&location) {
            Removes::File
        } else {
            Removes::Pointer
        };
        let target = Target {
            path: location.path.clone(),
            location,
        };
        Ok((target, removes))
    }

    /// Each file beneath the directory `path` names, for removing, with
    /// its own logical path, as `git rm -r` finds them; a file is itself.
    /// A pointer beneath is removed and not followed. A symlink beneath,
    /// or a name with no logical path, refuses them all.
    pub fn removal_targets(
        &mut self,
        path: &LogicalPath,
    ) -> Result<Vec<(LogicalPath, Target, Removes)>, WriteError> {
        let location = self.removable(path)?;
        if let Object::Blob(_) = location.object {
            let (target, removes) = self.removal_target(path)?;
            return Ok(vec![(path.clone(), target, removes)]);
        }
        let entries = (self.repository()).walk_under(&Source::WorkingTree, &location.path)?;
        entries
            .into_iter()
            .map(|entry| {
                let logical = entry.logical.ok_or_else(|| WriteError::Unnamed {
                    path: path.clone(),
                    file: entry.path.clone(),
                })?;
                let (target, removes) = self.removal_target(&logical)?;
                Ok((logical, target, removes))
            })
            .collect()
    }

    /// Where `path` leads, without following a pointer at the end, refused
    /// when it names nothing or is in a mounted board.
    fn removable(&mut self, path: &LogicalPath) -> Result<Location, WriteError> {
        let location = self
            .resolve_pointer(path)
            .map_err(|source| WriteError::Unremovable {
                path: path.clone(),
                source: Box::new(source),
            })?;
        if location.board.source != Source::WorkingTree {
            return Err(WriteError::RemoveThroughMount {
                path: path.clone(),
                mount: mount_passed(path, &location),
            });
        }
        Ok(location)
    }
}
