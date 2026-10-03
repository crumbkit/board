//! The one module that reads the environment, once.

use std::path::PathBuf;

/// What the binary learns from its environment.
#[derive(Debug)]
pub(crate) struct Context {
    /// Where the search for a board starts.
    pub(crate) start: PathBuf,
    /// What a mount's `~/` means.
    pub(crate) home: Option<PathBuf>,
}

impl Context {
    pub(crate) fn read() -> Self {
        Self {
            start: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            home: std::env::home_dir(),
        }
    }
}
