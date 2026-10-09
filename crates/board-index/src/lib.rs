//! The disposable `SQLite` index (docs/architecture/index.md). Only this crate
//! depends on rusqlite. It stores the facts it is given and decides no
//! answer: only when to prune, and which working-tree files a walk need not
//! hash again (git's racy-clean rule). Deleting it changes no answer, only
//! speed.

mod database;
mod extract;
mod open;
mod query;
mod schema;
mod store;
mod timing;

pub use crate::database::{DatabaseError, PluginDatabase};
pub use crate::extract::Unhandled;
pub use crate::open::{Fallback, Index, IndexError, Storage, location};
pub use crate::query::Rows;
pub use crate::schema::INDEX_VERSION;
pub use crate::store::{
    BlobFact, RefFact, Resolved, ValidationFact, View, Worktree, Writer, source_name,
};
pub use crate::timing::append_timing;

#[cfg(test)]
mod tests;
