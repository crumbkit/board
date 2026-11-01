//! Every operation as a typed function that returns its report
//! (docs/architecture.md, Commands). `board-cli` only parses arguments,
//! reads configuration and prints.

pub mod agent;
mod alias;
mod attachment;
mod board;
mod call;
mod check;
mod effects;
mod enable;
mod host;
mod index;
mod init;
mod known;
mod mount;
mod plugins;
mod put;
mod query;
mod read;
mod refs;
mod snapshot;
mod sync;
mod trace;
mod types;
mod ui;
mod verdict;

pub use board_index::{Budget, Fallback, append_timing, source_name};

pub use crate::attachment::{FetchError, MAX_INLINE_BYTES};
pub use crate::board::{Backend, Board, Config, OpenError, Store};
pub use crate::check::{CheckError, CheckReport, Checked, Detail};
pub use crate::effects::{AnswerReport, Asker, DecideReport, EffectError, Shown, Signing, answer};
pub use crate::enable::{EnableError, EnableReport, Kept};
pub use crate::host::{BuiltIn, Host, Limit, Plugin, PluginError, Run, Settings, Variables};
pub use crate::index::{RefreshError, Refreshed};
pub use crate::init::{InitError, InitReport, init};
pub use crate::known::{KnownBoards, Prefix, asker, open, unopened};
pub use crate::mount::{MountError, MountUpdateReport};
pub use crate::plugins::{
    CallFailed, Found, PluginEntry, PluginFindReport, PluginListReport, PluginState, Plugins,
    SchemaProblem,
};
pub use crate::put::{
    Expect, MAX_BATCH_LINES, PutError, PutReport, Refused, Removed, Removing, Unmatched, Vars,
    Written, folder_text,
};
pub use crate::query::{DEFAULT_LIMIT, MAX_LIMIT, QueryError, QueryReport};
pub use crate::read::{GetReport, Listed, LsReport, ReadError};
pub use crate::refs::{Absent, Fielded, Incoming, Missing, Outgoing, RefsError, RefsReport};
pub use crate::sync::SyncError;
pub use crate::trace::{Phase, Trace};
pub use crate::types::TypeError;
pub use crate::ui::{BindingError, Drawn, Failed, Page, PageError};
pub use crate::verdict::{Fault, SchemaFound};

#[cfg(test)]
mod tests;
