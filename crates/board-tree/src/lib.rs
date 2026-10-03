//! Every read of git objects and the working tree, resolution and walking.
//! Only this crate knows that trees, links and mounts exist; everything else
//! takes Locations.

mod address;
mod git;
mod resolve;
mod tree;
mod walk;
mod worktree;

#[cfg(test)]
mod tests;

pub use crate::git::{GitError, Repository};
pub use crate::resolve::{MAX_HOPS, ResolveError};
pub use crate::walk::{Change, Entry, Status};
