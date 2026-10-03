//! The one module that reads the environment, once.

use std::path::PathBuf;

use board_plugin::Environment;

/// What the binary learns from its environment.
#[derive(Debug)]
pub(crate) struct Context {
    /// Where the search for a board starts.
    pub(crate) start: PathBuf,
    /// What a mount's `~/` means.
    pub(crate) home: Option<PathBuf>,
    /// Where the index is kept: `$XDG_CACHE_HOME`, or `~/.cache`.
    pub(crate) cache: Option<PathBuf>,
    /// Where plugins are installed: `$XDG_DATA_HOME/board/plugins`, or
    /// `~/.local/share/board/plugins`.
    pub(crate) plugins: Option<PathBuf>,
    /// What a called plugin inherits.
    pub(crate) environment: Environment,
}

impl Context {
    #[expect(
        clippy::disallowed_methods,
        reason = "this module reads the environment, once"
    )]
    pub(crate) fn read() -> Self {
        let home = std::env::home_dir();
        // The XDG base directory rule: a relative path is ignored.
        let base = |variable: &str, default: &str| {
            std::env::var_os(variable)
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .or_else(|| home.as_ref().map(|home| home.join(default)))
        };
        let cache = base("XDG_CACHE_HOME", ".cache");
        let plugins = base("XDG_DATA_HOME", ".local/share").map(|data| data.join("board/plugins"));
        Self {
            start: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            home,
            cache,
            plugins,
            environment: Environment {
                path: std::env::var_os("PATH"),
                home: std::env::var_os("HOME"),
                lang: std::env::var_os("LANG"),
                cache: std::env::var_os("XDG_CACHE_HOME"),
                data: std::env::var_os("XDG_DATA_HOME"),
                depth: std::env::var_os("BOARD_CALL_DEPTH"),
            },
        }
    }
}
