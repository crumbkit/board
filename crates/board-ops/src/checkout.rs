//! What `board --checkout` refuses. Every refusal has the code
//! `board::cli::usage` and a hint that says what to run instead. A write the
//! run would make is refused where it takes the board's lock, by
//! `board_tree::WriteError::AtCheckout`; every other refusal is listed here.

use std::path::Path;

use board_model::{Code, Fields, PluginName, Refusal};

/// Something `--checkout` cannot do.
#[derive(Debug)]
pub enum Refused<'a> {
    /// The run starts in the board's own tree.
    OwnTree(&'a Path),
    /// The run starts in a repository no known board mounts, or one matched
    /// only by a mount of another clone or a repo record.
    NotMounted(&'a Path),
    /// The run starts in a checkout whose HEAD names no commit.
    NoCommit(&'a Path),
    /// A command that opens no board, such as `init` and `answer`, or one
    /// that serves the board past this run, such as `serve` and `ui`.
    Command(&'static str),
    /// A process plugin, whose own `board` runs read each mount at its pin.
    ProcessPlugin(&'a PluginName),
}

impl Refused<'_> {
    /// The refusal Board reports.
    pub fn refusal(&self) -> Box<Refusal> {
        let (message, hint) = match self {
            Self::OwnTree(start) => (
                not_a_checkout(
                    start,
                    "is in the board's own tree, which Board reads from the working tree",
                ),
                FROM_A_CHECKOUT.to_owned(),
            ),
            Self::NotMounted(start) => (
                not_a_checkout(
                    start,
                    "is in a repository no known board mounts; a mount of another clone, \
                     or a repo record, is read at its pin",
                ),
                FROM_A_CHECKOUT.to_owned(),
            ),
            Self::NoCommit(start) => (
                not_a_checkout(
                    start,
                    "has a HEAD that names no commit, such as an unborn branch",
                ),
                FROM_A_CHECKOUT.to_owned(),
            ),
            Self::Command(command) => (
                format!(
                    "--checkout reads a mount at its checkout's HEAD for one run, and \
                     `board {command}` opens no board or serves one past this run"
                ),
                format!("run `board {command}` without --checkout"),
            ),
            Self::ProcessPlugin(plugin) => (
                format!(
                    "--checkout runs only the plugins built into Board, and {plugin} is a \
                     process plugin whose own board runs read each mount at its pin"
                ),
                format!(
                    "call {plugin} without --checkout, or use a built-in plugin, such as \
                     controls, under --checkout"
                ),
            ),
        };
        Box::new(Refusal::new(Code::CliUsage, message, &Fields::default()).with_hint(hint))
    }
}

/// The hint for a run that did not start in a checkout of a mounted
/// repository.
const FROM_A_CHECKOUT: &str = "run `board --checkout` from a checkout of a repository a known \
                               board mounts, or run without --checkout to read each mount at \
                               its pin";

/// The message for a run from `start`, which `reason` says is not in a
/// checkout of a mounted repository.
fn not_a_checkout(start: &Path, reason: &str) -> String {
    format!(
        "--checkout reads a mounted repository at its checkout's HEAD, but {} {reason}",
        start.display()
    )
}
