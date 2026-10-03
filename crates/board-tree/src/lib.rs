//! Every read of git objects and the working tree, resolution and walking.
//! Only this crate knows that trees, links and mounts exist; everything else
//! takes Locations.

mod address;
mod git;
mod resolve;
mod tree;
mod trees;
mod walk;
mod worktree;
mod write;

#[cfg(test)]
mod tests;

pub use crate::git::{GitError, Repository};
pub use crate::resolve::{MAX_HOPS, ResolveError, Resolver};
pub use crate::trees::{Rows, TreeRow, TreeRows, walk_stored};
pub use crate::walk::{Entry, Status};
pub use crate::worktree::{Known, discover, is_repository_root};
pub use crate::write::{LOCK_WAIT, Lock, Staged, Target, WriteError};
