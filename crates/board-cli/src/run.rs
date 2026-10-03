//! The command line and what each command does with it.

use std::ffi::OsString;
use std::io::Read;

use board_model::{Code, LogicalPath, MAX_RECORD_BYTES, ObjectId};
use board_ops::{Board, DEFAULT_LIMIT, Expect, MAX_LIMIT};
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
    /// List the references a record makes and the records that reach it.
    Refs { path: LogicalPath },
    /// Check the record on stdin and write it, in canonical form, at a path.
    Put {
        #[arg(required_unless_present = "batch")]
        path: Option<LogicalPath>,
        /// The blob the file must hold, or `none` for a new file.
        #[arg(long, value_parser = expect)]
        expect: Option<Expect>,
        /// Read JSON Lines of `{path, expect, record}` and write them all or none.
        #[arg(long, conflicts_with_all = ["path", "expect"])]
        batch: bool,
    },
    /// Run one read-only SQL statement over the index.
    Query {
        sql: String,
        /// The most rows to return.
        #[arg(long, default_value_t = DEFAULT_LIMIT, value_parser = limit)]
        limit: usize,
    },
}

/// `none`, or a blob id in hex.
fn expect(text: &str) -> Result<Expect, String> {
    if text == "none" {
        return Ok(Expect::Absent);
    }
    ObjectId::from_hex(text)
        .map(Expect::Blob)
        .ok_or_else(|| "must be `none` or a blob id in lowercase hex".to_owned())
}

/// A row limit from 1 to [`MAX_LIMIT`].
fn limit(text: &str) -> Result<usize, String> {
    match text.parse::<usize>() {
        Ok(limit) if (1..=MAX_LIMIT).contains(&limit) => Ok(limit),
        _ => Err(format!("must be a whole number from 1 to {MAX_LIMIT}")),
    }
}

/// Runs one command line against the board found from `context`, reading a
/// record from `stdin` when the command takes one.
pub(crate) fn run(
    arguments: impl IntoIterator<Item = OsString>,
    context: &Context,
    stdin: &mut dyn Read,
) -> Output {
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
        Command::Check { path } => match board.check(path.as_ref(), context.cache.as_deref()) {
            Ok(report) => render::check(format, &report),
            Err(error) => render::error(format, error.code(), &error),
        },
        Command::Get { path } => match board.get(&path) {
            Ok(report) => render::get(format, &report),
            Err(error) => render::error(format, error.code(), &error),
        },
        Command::Put { batch: true, .. } => {
            match board.put_batch(&mut std::io::BufReader::new(stdin)) {
                Ok(report) => render::put(format, &report),
                Err(error) => render::put_error(format, &error),
            }
        }
        Command::Put {
            path: Some(path),
            expect,
            ..
        } => {
            let mut record = Vec::new();
            // One byte past the bound, so the parse sees the input is too large.
            if let Err(error) = stdin
                .take(MAX_RECORD_BYTES as u64 + 1)
                .read_to_end(&mut record)
            {
                return render::error(
                    format,
                    Code::RepoUnreadable,
                    &format!("stdin cannot be read: {error}"),
                );
            }
            match board.put(&path, expect.unwrap_or(Expect::Any), &record) {
                Ok(report) => render::put(format, &report),
                Err(error) => render::put_error(format, &error),
            }
        }
        Command::Put { path: None, .. } => unreachable!("clap requires a path without --batch"),
        Command::Refs { path } => match board.refs(&path, context.cache.as_deref()) {
            Ok(report) => render::refs(format, &report),
            Err(error) => render::error(format, error.code(), &error),
        },
        Command::Query { sql, limit } => match board.query(&sql, limit, context.cache.as_deref()) {
            Ok(report) => render::query(format, &report),
            Err(error) => render::error(format, error.code(), &error),
        },
        Command::Ls { path } => match board.ls(&path.unwrap_or_else(LogicalPath::root)) {
            Ok(report) => render::ls(format, &report),
            Err(error) => render::error(format, error.code(), &error),
        },
    }
}
