//! The one module that reads the environment, once.

use std::ffi::OsString;
use std::io::IsTerminal as _;
use std::path::{Path, PathBuf};

use board_model::Refusal;
use board_ops::{BuiltIn, KnownBoards};
use board_plugin::Environment;

/// What the binary learns from its environment.
#[derive(Debug)]
pub(crate) struct Context {
    /// Where the search for a board starts.
    pub(crate) start: PathBuf,
    /// What a mount's `~/` means.
    pub(crate) home: Option<PathBuf>,
    /// Where the index and timing log are kept: `$XDG_CACHE_HOME`, or `~/.cache`.
    pub(crate) cache: Option<PathBuf>,
    /// Where plugins are installed: `$XDG_DATA_HOME/board/plugins`, or
    /// `~/.local/share/board/plugins`.
    pub(crate) plugins: Option<PathBuf>,
    /// The file naming the user's known boards:
    /// `$XDG_CONFIG_HOME/board/config.json`, or `~/.config/board/config.json`.
    pub(crate) boards: Option<PathBuf>,
    /// The program that signs an approval: `.local/bin/attention-sign` in
    /// the home the passwd entry names, which no variable moves. A debug
    /// build, which every test runs, has none, so no test opens the owner's
    /// Touch ID window. `cargo install` makes a release build.
    pub(crate) signer: Option<OsString>,
    /// The allowed signers file an approval's signature must verify against:
    /// `.config/board/allowed_signers` in the same home.
    pub(crate) allowed_signers: Option<PathBuf>,
    /// What a called plugin inherits.
    pub(crate) environment: Environment,
    /// `BOARD_COMPLETE`, which names a shell to ask for completion.
    pub(crate) complete: Option<OsString>,
    /// Whether stdin is a terminal, which `call` does not wait to read.
    pub(crate) terminal: bool,
    /// `BROWSER`, the command `board ui open` opens a page with.
    pub(crate) browser: Option<OsString>,
    /// The running `board` binary, which `board ui open` starts the server with.
    pub(crate) binary: Option<PathBuf>,
    /// `_CLAP_COMPLETE_INDEX`: which word the bash or zsh cursor is on.
    /// `clap_complete` reads it, `_CLAP_IFS` and its other `_CLAP_*`
    /// variables from the environment itself, the one exception to this
    /// module owning the environment.
    pub(crate) cursor: Option<usize>,
    /// The plugins built into this binary, which the in-process tests leave
    /// out.
    pub(crate) built_in: BuiltIn,
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
        let config = base("XDG_CONFIG_HOME", ".config").map(|config| config.join("board"));
        let boards = config.map(|config| config.join("config.json"));
        let (signer, allowed_signers) = owner_home().as_deref().map(trust).unzip();
        let path = std::env::var_os("PATH");
        let current = std::env::current_dir().ok();
        let start = current.clone().unwrap_or_else(|| PathBuf::from("."));
        Self {
            start: start.clone(),
            home,
            cache,
            plugins,
            boards,
            signer: signer.filter(|_| cfg!(not(debug_assertions))),
            allowed_signers,
            environment: Environment {
                path,
                home: std::env::var_os("HOME"),
                lang: std::env::var_os("LANG"),
                tz: std::env::var_os("TZ"),
                cache: std::env::var_os("XDG_CACHE_HOME"),
                data: std::env::var_os("XDG_DATA_HOME"),
                config: std::env::var_os("XDG_CONFIG_HOME"),
                depth: std::env::var_os("BOARD_CALL_DEPTH"),
                timings: std::env::var_os("BOARD_TIMINGS"),
                caller: current.map(PathBuf::into_os_string),
                day_start: None,
                plugin: std::env::var_os(board_plugin::PLUGIN_VARIABLE),
                inherited: std::env::vars_os().collect(),
            },
            complete: std::env::var_os("BOARD_COMPLETE"),
            terminal: std::io::stdin().is_terminal(),
            browser: std::env::var_os("BROWSER"),
            binary: std::env::current_exe().ok(),
            cursor: std::env::var("_CLAP_COMPLETE_INDEX")
                .ok()
                .and_then(|index| index.parse().ok()),
            built_in: board_plugins::BUILT_IN,
        }
    }

    /// The known boards the file `boards` names, none when there is no file.
    pub(crate) fn known(&self) -> Result<KnownBoards, Box<Refusal>> {
        let Some(file) = &self.boards else {
            return Ok(KnownBoards::default());
        };
        KnownBoards::parse(file, std::fs::read(file))
    }

    /// The root of the personal board the known boards file names.
    pub(crate) fn personal(&self) -> Result<PathBuf, Box<Refusal>> {
        self.known()?.personal(self.home.as_deref())
    }
}

/// The home directory the passwd entry of this process's user names.
/// `HOME` and the XDG variables are the caller's to set, so they never
/// decide what an approval trusts.
fn owner_home() -> Option<PathBuf> {
    let user = nix::unistd::User::from_uid(nix::unistd::getuid()).ok()??;
    Some(user.dir).filter(|dir| dir.is_absolute())
}

/// The allowed signers file, relative to the owner's home.
pub(crate) const ALLOWED_SIGNERS: &str = ".config/board/allowed_signers";

/// The signer and the allowed signers file in the owner's `home`.
fn trust(home: &Path) -> (OsString, PathBuf) {
    let signer = home.join(".local/bin/attention-sign").into_os_string();
    (signer, home.join(ALLOWED_SIGNERS))
}

#[cfg(test)]
mod tests;
