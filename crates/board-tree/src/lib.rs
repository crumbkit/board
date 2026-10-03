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

#[cfg(test)]
mod tests;

pub use crate::git::{GitError, Repository};
pub use crate::resolve::{MAX_HOPS, ResolveError, Resolver};
pub use crate::trees::{TreeRow, TreeRows};
pub use crate::walk::{Entry, Status};
pub use crate::worktree::{Known, discover, is_repository_root};
