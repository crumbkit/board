//! The effects commands: `decide` and `answer`.

use board_model::{Answer, Refusal};
use board_ops::{Board, EffectError};

use crate::config::Context;
use crate::known;
use crate::render::{self, Printer};
use crate::report::Output;
use crate::run::{Given, Prefix, read};
use crate::timing::Ran;

/// `board decide`: the authority `op` has at `path`, read from `prefix`.
pub(crate) fn decide(
    board: &Board,
    printer: Printer,
    prefix: Option<&Prefix>,
    op: &str,
    path: &Given,
) -> Output {
    let decided = read(prefix, path, "decide")
        .and_then(|path| board.authority(op, &path).map_err(|error| refusal(&error)));
    match decided {
        Ok(report) => render::decide(printer, &report),
        Err(refusal) => render::refused(printer, *refusal),
    }
}

/// The arguments of `board answer`.
#[derive(Debug, clap::Args)]
pub(crate) struct Answering {
    /// The pending effect's id, 64 hex digits.
    id: String,
    /// approve lets the op act; refuse refuses it.
    answer: Answer,
    /// Why, kept in the seen record.
    #[arg(long)]
    reason: Option<String>,
    /// Also save a preference for the op, exact path, plugin and board; a put's approval cannot be saved.
    #[arg(long)]
    save_rule: bool,
}

/// `board answer`, which needs no board but the personal one.
pub(crate) fn answer(context: &Context, printer: Printer, answering: Answering) -> Output {
    let Answering {
        id,
        answer,
        reason,
        save_rule,
    } = answering;
    let answered = known::asker(context, true, false).and_then(|asker| {
        board_ops::answer(&asker, &id, answer, reason, save_rule).map_err(|error| refusal(&error))
    });
    match answered {
        Ok(report) => render::answer(printer, &report),
        Err(refusal) => render::refused(printer, *refusal),
    }
}

/// Refuses, under `BOARD_PLUGIN`, the command `ran` names: a write the put
/// check does not cover, which only the owner runs. `None` for the owner.
pub(crate) fn from_plugin(context: &Context, ran: &Ran) -> Option<Box<Refusal>> {
    let plugin = match known::plugin(context) {
        Ok(plugin) => plugin?,
        Err(refusal) => return Some(refusal),
    };
    let what = format!("run board {}", ran.command.as_deref().unwrap_or_default());
    let error = EffectError::FromPlugin {
        plugin,
        path: None,
        what,
    };
    Some(refusal(&error))
}

fn refusal(error: &EffectError) -> Box<Refusal> {
    Box::new(error.refusal())
}
