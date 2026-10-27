//! Where a removal lands (docs/architecture/writes/removing.md).

use board_model::{LogicalPath, Object, Source};

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
}
