//! Every operation as a typed function that returns its report
//! (docs/architecture.md, Commands). `board-cli` only parses arguments,
//! reads configuration and prints.

mod board;
mod check;
mod index;
mod mounts;
mod query;
mod read;
mod refs;

pub use board_index::{Fallback, source_name};

pub use crate::board::{Board, Config, OpenError};
pub use crate::check::{CheckError, CheckReport, Checked};
pub use crate::index::{RefreshError, Refreshed};
pub use crate::query::{DEFAULT_LIMIT, MAX_LIMIT, QueryError, QueryReport};
pub use crate::read::{GetReport, Listed, LsReport, ReadError};
pub use crate::refs::{Incoming, Outgoing, RefsError, RefsReport};

#[cfg(test)]
mod tests;
