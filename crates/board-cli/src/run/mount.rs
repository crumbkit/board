//! `board mount add` and `board mount update`: their arguments and the
//! mount writes they ask for (docs/architecture/paths.md, Pointers).

use std::path::Path;

use board_model::LogicalPath;
use board_ops::Board;
use clap::Subcommand;

use crate::render::{self, Printer};
use crate::report::Output;

/// `board mount --help` after its options.
pub(super) const HELP: &str = "\
A mount holds `repo`, `follow` and `commit`: a board/mount/v1 record, or the
`checkout` of a board/repo/v1 record, which also names the repository's
`remote`. Board reads the mounted repository at `commit`. `mount add` writes
a mount of a local clone; `mount update` moves `commit` to what `follow`
names.

Examples:
  board mount add /team ~/team-board
  board mount update /vendor/lib";

#[derive(Debug, Subcommand)]
pub(crate) enum MountCommand {
    /// Mount a local clone at a path that holds nothing yet.
    #[command(after_help = "\
Writes a board/mount/v1 record at the path, through put with --expect none.
`follow` is the branch checked out in the clone, unless --follow names
another ref. A write through the mount needs `follow` to name a local
branch. `commit` is the commit `follow` names now. `repo` is stored as the
shell passes it, so ~/team-board is stored as an absolute path. A relative
`repo` is made absolute against the directory the run started in.

Examples:
  board mount add /team ~/team-board
  board mount add /team ~/team-board --follow main")]
    Add {
        /// The mount's logical path, such as /team.
        path: LogicalPath,
        /// The directory of a local git clone, such as ~/team-board.
        repo: String,
        /// The ref `mount update` advances the mount to, such as main.
        #[arg(long)]
        follow: Option<String>,
    },
    /// Advance the mount at a path, which is not followed, to its `follow` ref.
    #[command(after_help = "\
Examples:
  board mount update /vendor/lib")]
    Update {
        /// The mount's logical path, such as /vendor/lib.
        path: LogicalPath,
    },
}

/// Runs `command` against `board`, reading a relative `repo` against
/// `start`, the directory the run started in.
pub(super) fn run(board: &Board, printer: Printer, start: &Path, command: MountCommand) -> Output {
    match command {
        MountCommand::Add { path, repo, follow } => {
            match board.mount_add(&path, (&repo, start), follow) {
                Ok((mount, report)) => render::mount_add(printer, &mount, &report),
                Err(error) => render::refused(printer, error.refusal()),
            }
        }
        MountCommand::Update { path } => match board.mount_update(&path, board_tree::LOCK_WAIT) {
            Ok(report) => render::mount_update(printer, &report),
            Err(error) => render::refused(printer, error.refusal()),
        },
    }
}
