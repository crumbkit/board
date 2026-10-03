//! Every read of git objects, resolution and walking. Only this crate knows
//! that trees, links and mounts exist; everything else takes Locations.

mod address;
mod git;
mod resolve;
mod walk;

#[cfg(test)]
mod scenario;
#[cfg(test)]
mod tests;

pub use crate::git::{GitError, Repository};
pub use crate::resolve::ResolveError;
pub use crate::walk::{Entry, Status};
