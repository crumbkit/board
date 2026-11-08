//! `board rm`: its arguments and the removal they ask for.

use std::time::Duration;

use board_ops::{Board, Expect};

use super::{Given, Prefix};
use crate::render::{self, Printer};
use crate::report::Output;
use crate::values::{duration, expect, message, removal};

/// `rm`'s arguments.
#[derive(Debug, clap::Args)]
pub(crate) struct Removal {
    /// The file's logical path, such as /tasks/a, or a from /tasks; a JSON file's leaves out .json.
    path: Given,
    /// The blob the file must hold.
    #[arg(long, value_parser = expect)]
    expect: Option<Expect>,
    /// Remove every file beneath a directory.
    #[arg(short = 'r', long, conflicts_with = "expect")]
    recursive: bool,
    /// Report a path that names nothing as absent.
    #[arg(long)]
    ignore_unmatch: bool,
    /// The commit message.
    ///
    /// The removal's commit takes this message as written. Without it, the message is
    /// `board rm <PATH>`, or `board rm -r <PATH>` with -r.
    #[arg(short = 'm', long, value_name = "TEXT", value_parser = message)]
    message: Option<String>,
    /// How long to wait for the board's lock, with a unit: 500ms, 30s, 2m or 1h.
    #[arg(long, value_name = "DURATION", value_parser = duration, default_value = "5s")]
    wait: Duration,
}

/// Removes what `asked` names, read from `prefix`, and reports it.
pub(crate) fn remove(
    board: &Board,
    printer: Printer,
    prefix: Option<&Prefix>,
    asked: Removal,
) -> Output {
    let Removal {
        path,
        expect,
        recursive,
        ignore_unmatch,
        message,
        wait,
    } = asked;
    let path = match crate::mistaken::file_path(prefix, &path, "rm", recursive) {
        Ok(path) => path,
        Err(refusal) => return render::refused(printer, *refusal),
    };
    let (removing, unmatched) = removal(recursive, expect, ignore_unmatch);
    match board.remove_waiting(&path, removing, unmatched, message.as_deref(), wait) {
        Ok(report) => render::put(printer, &report, None),
        Err(error) => render::refused(printer, error.refusal()),
    }
}
