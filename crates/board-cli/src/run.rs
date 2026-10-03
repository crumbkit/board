//! The command line and what each command does with it.

use std::ffi::OsString;

use board_model::LogicalPath;
use board_ops::Board;
use clap::error::ErrorKind;
use clap::{Parser, Subcommand};

use crate::config::Context;
use crate::render::{self, Format};
use crate::report::Output;

#[derive(Debug, Parser)]
#[command(
    name = env!("CARGO_BIN_NAME"),
    about = "Board: a fast, validated view of a git tree."
)]
struct Cli {
    /// Print one JSON document instead of text.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Check every entry, or those at or beneath a path; fail if any is degraded.
    Check { path: Option<LogicalPath> },
    /// Print the record at a path, following pointers.
    Get { path: LogicalPath },
    /// List the directory at a path, without following pointers inside it.
    Ls { path: Option<LogicalPath> },
}

/// Runs one command line against the board found from `context`.
pub(crate) fn run(arguments: impl IntoIterator<Item = OsString>, context: &Context) -> Output {
    let cli = match Cli::try_parse_from(arguments) {
        Ok(cli) => cli,
        Err(error) => {
            let text = error.render().to_string().into_bytes();
            return match error.kind() {
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => Output {
                    stdout: text,
                    ..Output::default()
                },
                _ => Output {
                    stderr: text,
                    code: 2,
                    ..Output::default()
                },
            };
        }
    };
    let format = if cli.json { Format::Json } else { Format::Text };
    let board = match Board::open(&context.start, context.home.clone()) {
        Ok(board) => board,
        Err(error) => return render::error(format, error.code(), &error),
    };
    match cli.command {
        Command::Check { path } => match board.check(path.as_ref()) {
            Ok(report) => render::check(format, &report),
            Err(error) => render::error(format, error.code(), &error),
        },
        Command::Get { path } => match board.get(&path) {
            Ok(report) => render::get(format, &report),
            Err(error) => render::error(format, error.code(), &error),
        },
        Command::Ls { path } => match board.ls(&path.unwrap_or_else(LogicalPath::root)) {
            Ok(report) => render::ls(format, &report),
            Err(error) => render::error(format, error.code(), &error),
        },
    }
}
