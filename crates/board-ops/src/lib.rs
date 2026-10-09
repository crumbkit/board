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
mod plugins;
mod put;
mod query;
mod read;
mod refs;
mod snapshot;
mod trace;
mod verdict;

pub use board_index::{Fallback, append_timing, source_name};

pub use crate::board::{Board, Config, OpenError};
pub use crate::call::Called;
pub use crate::check::{CheckError, CheckReport, Checked, Detail};
pub use crate::enable::{EnableError, EnableReport};
pub use crate::extraction::{CallFailed, Plugins};
pub use crate::index::{RefreshError, Refreshed};
pub use crate::init::{InitError, InitReport, init};
pub use crate::mount::{MountError, MountUpdateReport};
pub use crate::plugins::{
    Found, Plugin, PluginFindReport, PluginListReport, PluginState, SchemaProblem,
};
pub use crate::put::{Expect, MAX_BATCH_LINES, PutError, PutReport, Refused, Written};
pub use crate::query::{DEFAULT_LIMIT, MAX_LIMIT, PluginFailed, QueryError, QueryReport};
pub use crate::read::{GetReport, Listed, LsReport, ReadError};
pub use crate::refs::{Incoming, Missing, Outgoing, RefsError, RefsReport};
pub use crate::trace::{Phase, Trace};
pub use crate::verdict::{Fault, SchemaFound};

#[cfg(test)]
mod tests;
