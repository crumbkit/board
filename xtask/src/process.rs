//! The one owner of xtask's environment reads and child processes.
#![expect(
    clippy::disallowed_methods,
    reason = "xtask reads its environment and starts children only here"
)]

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The environment variable `name`, if set.
pub(crate) fn var(name: &str) -> Option<OsString> {
    std::env::var_os(name)
}

/// A command that runs `program`.
pub(crate) fn command(program: impl AsRef<OsStr>) -> Command {
    Command::new(program)
}

/// The `cargo` that ran xtask, or the one on `PATH`.
pub(crate) fn cargo() -> Command {
    command(var("CARGO").unwrap_or_else(|| "cargo".into()))
}

/// Where cargo builds the workspace at `root`.
pub(crate) fn target(root: &Path) -> PathBuf {
    var("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), PathBuf::from)
}
