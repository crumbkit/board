//! `board sync`: its arguments and the sync they ask for
//! (docs/architecture/writes.md, Sync).

use board_ops::Board;

use crate::render::{self, Printer};
use crate::report::Output;

/// `sync`'s arguments.
#[derive(Debug, clap::Args)]
pub(crate) struct Syncing {
    /// Fetch and merge, and leave the push for later.
    #[arg(long)]
    no_push: bool,
    /// Merge a remote branch that was rewritten since the last fetch, which otherwise stops the sync. The clone's commits, including any the rewrite removed, are pushed again, so a deliberate rewrite such as removing a secret needs a fresh clone instead.
    #[arg(long)]
    accept_rewrite: bool,
}

/// Syncs the board as `asked`.
pub(super) fn run(board: &Board, printer: Printer, asked: &Syncing) -> Output {
    let deadline = board_tree::GIT_DEADLINE;
    match board.sync_accepting_rewrite(!asked.no_push, asked.accept_rewrite, deadline) {
        Ok(report) => render::sync(printer, &report),
        Err(error) => render::refused(printer, error.refusal()),
    }
}
