//! The disposable `SQLite` index (docs/architecture.md, Index). Only this crate
//! depends on rusqlite. It stores the facts it is given and decides nothing:
//! deleting it changes no answer, only speed.

mod database;
mod extract;
mod open;
mod query;
mod schema;
mod store;

pub use crate::database::PluginDatabase;
pub use crate::extract::Unhandled;
pub use crate::open::{Fallback, Index, IndexError, Storage, location};
pub use crate::query::Rows;
pub use crate::schema::INDEX_VERSION;
pub use crate::store::{
    BlobFact, MountRow, RefFact, Resolved, ValidationFact, WorktreeRow, Writer, source_name,
};

#[cfg(test)]
mod tests;
