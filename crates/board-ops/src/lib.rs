//! Every operation as a typed function that returns its report
//! (docs/architecture.md, Commands). `board-cli` only parses arguments,
//! reads configuration and prints.

mod board;
mod call;
mod check;
mod enable;
mod extraction;
mod index;
mod init;
mod mount;
mod mounts;
mod plugins;
mod put;
mod query;
mod read;
mod refs;

pub use board_index::{Fallback, source_name};

pub use crate::board::{Board, Config, OpenError};
pub use crate::call::Called;
pub use crate::check::{CheckError, CheckReport, Checked};
pub use crate::enable::{EnableError, EnableReport};
pub use crate::extraction::{CallFailed, Plugins};
pub use crate::index::{RefreshError, Refreshed};
pub use crate::init::{InitError, InitReport, init};
pub use crate::mount::{MountError, MountUpdateReport};
pub use crate::plugins::{Found, Plugin, PluginFindReport, PluginListReport, PluginState};
pub use crate::put::{Expect, MAX_BATCH_LINES, PutError, PutReport, Refusal, Refused, Written};
pub use crate::query::{DEFAULT_LIMIT, Known, MAX_LIMIT, PluginFailed, QueryError, QueryReport};
pub use crate::read::{GetReport, Listed, LsReport, ReadError};
pub use crate::refs::{Incoming, Outgoing, RefsError, RefsReport};

#[cfg(test)]
mod tests;
