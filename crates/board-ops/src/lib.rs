//! Every operation as a typed function that returns its report
//! (docs/architecture.md, Commands). `board-cli` only parses arguments,
//! reads configuration and prints.

mod board;
mod check;
mod index;
mod read;

pub use board_index::Fallback;

pub use crate::board::{Board, Config, OpenError};
pub use crate::check::{CheckError, CheckReport, Checked};
pub use crate::index::{RefreshError, Refreshed};
pub use crate::read::{GetReport, Listed, LsReport, ReadError};

#[cfg(test)]
mod tests;
