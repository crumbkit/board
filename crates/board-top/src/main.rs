//! The `board-top` command.

use std::path::PathBuf;
use std::process::ExitCode;

use board_top::app::{Stream, emit, live, once};
use board_top::source::{Source, load_config};
use clap::Parser;

/// Shows the changes in play on a board, grouped by the work map, with how
/// long each has been in its phase and how much is in play against a limit.
/// It reads the board and writes nothing. Press q to quit.
#[derive(Parser, Debug)]
#[command(name = "board-top", version)]
struct Args {
    /// The board directory, as `board -C` takes it.
    #[arg(short = 'C', long, value_name = "DIR", default_value = ".")]
    board: PathBuf,
    /// The config file that sets `limit`; `~/.config/board-top/config.toml` when left out.
    #[arg(long, value_name = "FILE")]
    config: Option<PathBuf>,
    /// Draw one frame as plain text on standard output and exit.
    #[arg(long)]
    once: bool,
    /// With --once, the width of the frame in columns.
    #[arg(long, default_value_t = 120, value_name = "COLUMNS")]
    width: u16,
    /// With --once, the height of the frame in lines.
    #[arg(long, default_value_t = 50, value_name = "LINES")]
    height: u16,
}

fn default_config() -> PathBuf {
    std::env::home_dir()
        .unwrap_or_default()
        .join(".config/board-top/config.toml")
}

fn run(args: &Args) -> Result<(), String> {
    let config = load_config(&args.config.clone().unwrap_or_else(default_config))?;
    let source = Source {
        board: args.board.clone(),
        config,
    };
    if args.once {
        emit(Stream::Out, &once(&source, args.width, args.height)?);
        Ok(())
    } else {
        live(&source)
    }
}

fn main() -> ExitCode {
    let args = Args::parse();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            emit(Stream::Err, &format!("board-top: {error}"));
            ExitCode::FAILURE
        }
    }
}
