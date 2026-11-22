//! `board repo add`: its arguments and the repository record it writes
//! (docs/architecture/paths/repos.md).

use std::path::{Path, PathBuf};

use board_ops::Board;
use clap::Subcommand;

use crate::render::{self, Printer};
use crate::report::Output;

/// `board repo --help` after its options.
pub(super) const HELP: &str = "\
A repository record, board/repo/v2, holds one field, `remote`: the
repository's remote in identity form, such as crumbkit/board for GitHub or
gitlab.com/example-org/app for another host. It lives at
/repositories/<remote> in every board whose records name the repository.

Examples:
  board repo add git@github.com:crumbkit/board.git
  board repo add --from .";

#[derive(Debug, Subcommand)]
pub(crate) enum RepoCommand {
    /// Write a repository's record under /repositories, once.
    #[command(after_help = "\
Takes a remote in any form git reads: owner/name, an https or ssh URL, or
git@host:path. --from reads the origin remote of the clone that holds a
directory instead. It writes the record through put with --expect none and
prints its path. When the record exists it writes nothing and says
unchanged. A local path or file:// URL names no repository and is refused.

Examples:
  board repo add git@github.com:crumbkit/board.git
  board repo add https://gitlab.com/example-org/app
  board repo add --from .")]
    Add {
        /// The repository's remote, such as git@github.com:crumbkit/board.git.
        #[arg(required_unless_present = "from", conflicts_with = "from")]
        remote: Option<String>,
        /// A directory in a clone whose origin remote names the repository.
        #[arg(long, value_name = "DIR")]
        from: Option<PathBuf>,
    },
}

/// Runs `command` against `board`, reading a relative `--from` against
/// `start`, the directory the run started in.
pub(super) fn run(board: &Board, printer: Printer, start: &Path, command: RepoCommand) -> Output {
    let RepoCommand::Add { remote, from } = command;
    let added = match (remote, from) {
        (Some(remote), _) => board.repo_add(&remote),
        (None, Some(from)) => board.repo_add_from(&start.join(from)),
        (None, None) => unreachable!("clap requires a remote or --from"),
    };
    match added {
        Ok(added) => render::repo_add(printer, &added),
        Err(error) => render::refused(printer, error.refusal()),
    }
}
