//! The one owner of xtask's environment reads and child processes.
#![expect(
    clippy::disallowed_methods,
    reason = "xtask reads its environment and starts children only here"
)]

use std::ffi::{OsStr, OsString};
use std::process::Command;

/// The environment variable `name`, if set.
pub(crate) fn var(name: &str) -> Option<OsString> {
    std::env::var_os(name)
}

/// A command that runs `program`.
pub(crate) fn command(program: impl AsRef<OsStr>) -> Command {
    Command::new(program)
}
