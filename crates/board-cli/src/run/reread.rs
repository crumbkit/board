//! What Board does with a command line before it runs it: reading a
//! refused one again, and asking the owner to admit what a command writes.

use std::ffi::OsString;

use board_model::Refusal;
use board_ops::{Board, Bulk};

use super::mount::MountCommand;
use super::{Command, Putting, run};
use crate::config::Context;
use crate::effects;
use crate::render::{self, Printer};
use crate::report::Output;
use crate::stdin::Stdin;
use crate::timing::Ran;

/// The output of a refused command line that Board reads again: one that
/// gives too many paths, starts with an alias, guesses a word or a flag's
/// spelling, or makes a known mistake.
pub(super) fn reread(
    arguments: &[OsString],
    context: &Context,
    stdin: &mut dyn Stdin,
    ran: &mut Ran,
    refused: &crumbkit::Refused,
) -> Option<Output> {
    if let Some(output) = render::too_many_paths(refused) {
        return Some(output);
    }
    let Some(resolved) = crate::alias::after(arguments, context, refused) else {
        if let Some(meant) = crate::guessed::meant(arguments, refused) {
            return Some(run(meant, context, stdin, ran));
        }
        return crate::mistaken::hinted(arguments, refused);
    };
    Some(match resolved {
        Ok(called) => run(called, context, stdin, ran),
        Err(refusal) => {
            let printer = Printer {
                view: refused.run().clone(),
            };
            render::refused(printer, *refusal)
        }
    })
}

/// Lets `board` ask the owner to admit what `command` writes, and marks a
/// `put --bulk` as a bulk commit, refusing a reason a trailer cannot hold.
/// Only a plugin's decide writes in the personal board.
pub(super) fn ask(
    board: &mut Board,
    context: &Context,
    command: &Command,
) -> Result<(), Box<Refusal>> {
    if let Command::Put(Putting {
        bulk: Some(reason), ..
    }) = command
    {
        board.mark_bulk(Bulk::new(reason).map_err(|error| Box::new(error.refusal()))?);
    }
    let personal = match command {
        Command::Put(_)
        | Command::Rm(_)
        | Command::Mount(MountCommand::Add { .. })
        | Command::Repo(_) => false,
        Command::Decide(_) => context.environment.plugin.is_some(),
        _ => return Ok(()),
    };
    board.ask_as(effects::asker(context, personal)?);
    Ok(())
}
