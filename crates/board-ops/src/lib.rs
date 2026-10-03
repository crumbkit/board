//! Every operation as a typed function that returns its report
//! (docs/architecture.md, Commands). `board-cli` only parses arguments,
//! reads configuration and prints.

mod board;
mod check;

pub use crate::board::{Board, Config, OpenError};
pub use crate::check::{CheckError, CheckReport, Checked};

#[cfg(test)]
mod tests;
