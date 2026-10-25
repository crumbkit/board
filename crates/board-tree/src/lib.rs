//! Every read of git objects and the working tree, resolution and walking.
//! Only this crate knows that trees, links and mounts exist; everything else
//! takes Locations.

mod address;
mod git;
mod mounts;
mod repositories;
mod resolve;
mod symlink;
mod through;
mod transport;
mod tree;
mod trees;
mod walk;
mod worktree;
mod write;

#[cfg(test)]
mod tests;

pub use crate::git::{Contents, GitError, Repository, latest};
pub use crate::mounts::{
    Cache, MountPoint, Mounted, Pin, PinError, Place, Reached, Scopes, in_scope, within,
};
pub use crate::repositories::Repositories;
pub use crate::resolve::{Ancestor, InCommit, MAX_HOPS, ResolveError, Resolver};
pub use crate::symlink::Refused;
pub use crate::through::{Inside, Through};
pub use crate::transport::{GIT_DEADLINE, Upstream};
pub use crate::trees::{Rows, TreeRow, TreeRows, walk_stored};
pub use crate::walk::{Beneath, Entry, Link, Name, Status};
pub use crate::worktree::{
    Directory, Known, Prepare, Reader, Working, discover, is_repository_root, repository_root,
};
pub use crate::write::outside::write_outside;
pub use crate::write::sync::{ATTEMPTS, Replayed, SyncError, Synced};
pub use crate::write::{LOCK_WAIT, Lock, Staged, Target, WriteError};
