//! The disposable `SQLite` index (docs/architecture.md, Index). Only this crate
//! depends on rusqlite. It stores the facts it is given and decides nothing:
//! deleting it changes no answer, only speed.

mod open;
mod schema;
mod store;

pub use crate::open::{Fallback, Index, IndexError, Storage, location};
pub use crate::schema::INDEX_VERSION;
pub use crate::store::{BlobFact, RefFact, ValidationFact, Writer};

#[cfg(test)]
mod tests;
