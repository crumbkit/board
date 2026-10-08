//! Every read of git objects and the working tree, resolution and walking.
//! Only this crate knows that trees, links and mounts exist; everything else
//! takes Locations.

mod address;
mod git;
mod mounts;
mod resolve;
mod tree;
mod trees;
mod walk;
mod worktree;
mod write;

#[cfg(test)]
mod tests;

pub use crate::git::{Contents, GitError, Repository, latest};
pub use crate::mounts::{Cache, MountPoint, Mounted, Pin, PinError, Reached, Scopes, in_scope};
pub use crate::resolve::{Ancestor, MAX_HOPS, ResolveError, Resolver};
pub use crate::trees::{Rows, TreeRow, TreeRows, walk_stored};
pub use crate::walk::{Beneath, Entry, Status};
pub use crate::worktree::{Known, Working, discover, is_repository_root, repository_root};
pub use crate::write::{LOCK_WAIT, Lock, Staged, Target, WriteError};
