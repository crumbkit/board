//! The disposable `SQLite` index (docs/architecture/index.md). Only this crate
//! depends on rusqlite. It stores the facts it is given and decides no
//! answer: only when to prune, and which working-tree files a walk need not
//! hash again (git's racy-clean rule). Deleting it changes no answer, only
//! speed.

mod decisions;
mod directories;
mod mark;
mod open;
mod query;
mod resolutions;
mod schema;
mod server;
mod slots;
mod state;
mod store;
mod timing;
mod values;
mod worktree;

pub use crate::decisions::KeptRow;
pub use crate::mark::{Unmarked, take_unmarked};
pub use crate::open::{Fallback, Index, IndexError, Storage, location};
pub use crate::query::{Budget, Rows};
pub use crate::resolutions::{KeptReferences, Reached};
pub use crate::schema::INDEX_VERSION;
pub use crate::server::{
    empty_directory, read_server, remove_opener, remove_server, server_file, write_opener,
    write_server, write_statement,
};
pub use crate::slots::{Input, Slot};
pub use crate::state::{
    append_state, lock_state, remove_scratch, remove_state, replace_file, scratch_directory,
    write_state,
};
pub use crate::store::{
    BlobFact, Decided, RefFact, Resolved, ValidationFact, View, Writer, source_name,
};
pub use crate::timing::{
    Summarise, Timings, append_timing, read_timings, stamp_timing, timing_stamped,
};
pub use crate::values::{cache, cached};
pub use crate::worktree::{StoredFacts, Worktree};

#[cfg(test)]
mod tests;
