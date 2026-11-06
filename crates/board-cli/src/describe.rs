//! `board commands list`, `board schemas list` and `board schemas show`,
//! which describe Board's own commands and the documents they print. They
//! run in any directory, and in a board they also describe the capabilities
//! of the plugins it enables.

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Instant;

use board_model::{Code, Refusal, RefusalCode};
use board_ops::{MountsAt, Trace};
use clap::Subcommand;
use crumbkit::Sink;
use crumbkit::commands::{self, COMMANDS_LIST, CommandsList};
use crumbkit::help::Guide;
use crumbkit::schemas::{self, SCHEMAS_LIST, SCHEMAS_SHOW, SchemasList, SchemasShow};

use crate::config::Context;
use crate::graft;
use crate::render::{GUIDE, Printer};
use crate::report::Output;

/// The examples of `board commands`.
pub(crate) const COMMANDS_HELP: &str = "\
`board commands list` runs in any directory. In a board it also lists
`call <plugin>.<capability>` for each capability of each plugin the board
enables, and notes on stderr a plugin it cannot read. `board plugin find`
shows the capabilities' schemas.

Examples:
  board commands list
  board commands list plugin";

/// The examples of `board schemas`.
pub(crate) const SCHEMAS_HELP: &str = "\
Examples:
  board schemas list
  board schemas show board/check-report/v2";

/// The commands that describe Board's commands.
#[derive(Debug, Subcommand)]
pub(crate) enum CommandsCommand {
    /// List a branch of Board's commands with their arguments, examples and effects.
    #[command(after_help = "\
Examples:
  board commands list
  board commands list plugin
  board commands list put --json --full")]
    List(CommandsList),
}

/// The commands that describe the JSON documents Board prints.
#[derive(Debug, Subcommand)]
pub(crate) enum SchemasCommand {
    /// List the JSON documents that Board prints, with a description of each.
    #[command(after_help = "\
Examples:
  board schemas list
  board schemas list --json")]
    List(SchemasList),
    /// Print the JSON Schema of each document.
    #[command(after_help = "\
Examples:
  board schemas show board/check-report/v2")]
    Show(SchemasShow),
}

/// The guide and the parsed view of a listing command line `arguments`,
/// with `call <plugin>.<capability>` for each capability of each plugin that
/// the board at `start` enables, and a note for each enabled plugin it
/// cannot read. Only `board commands` and `board schemas` read plugin
/// manifests for it, and `trace` times the reading as `plugin.list`.
/// Outside a board they describe no capability, and their view stays
/// `view`; a board that does not open gets a note with the code why.
pub(crate) fn grafted(
    arguments: &[OsString],
    context: &Context,
    start: Result<PathBuf, Box<Refusal>>,
    view: crumbkit::Output,
    trace: &mut Trace,
) -> (Guide, Vec<String>, crumbkit::Output) {
    let opening = Instant::now();
    let opened = start.and_then(|start| context.open(&start, MountsAt::Pins));
    trace.add("open", opening);
    let mut board = match opened {
        Ok((board, _)) => board,
        Err(refusal) if refusal.code == RefusalCode::Board(Code::ConfigNotFound) => {
            return (GUIDE, Vec::new(), view);
        }
        Err(refusal) => {
            let note = format!(
                "{}: the board cannot be opened, so no plugin capability is listed: {}",
                refusal.code.as_str(),
                refusal.message
            );
            return (GUIDE, vec![note], view);
        }
    };
    board.build_in(context.built_in.0);
    let listing = Instant::now();
    let (grafted, notes) = graft::enabled(&board, context.plugins.as_deref());
    trace.add("plugin.list", listing);
    trace.count("capabilities", grafted.len());
    let guide = graft::guide(&grafted);
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let sink = &mut Sink::new(&mut stdout, &mut stderr);
    let command = graft::command(&grafted);
    let parsed =
        crumbkit::try_parse_to::<graft::Parsed, _, _, _, _>(sink, command, &guide, arguments);
    // The line parsed against Board's own tree, so it parses with more leaves.
    let view = parsed.map_or(view, |(_, view)| view);
    (guide, notes, view)
}

/// `board commands <command>`, with the commands of `guide` and `notes`
/// on the plugins whose capabilities it leaves out.
pub(crate) fn commands(
    printer: Printer,
    guide: &Guide,
    command: &CommandsCommand,
    notes: Vec<String>,
) -> Output {
    let CommandsCommand::List(args) = command;
    let mut view = printer.view;
    let listed = commands::list(&mut view, guide, args);
    noted(Printer { view }.kit(COMMANDS_LIST, listed), notes)
}

/// `output` with each of `notes` on its own line of stderr.
fn noted(mut output: Output, notes: Vec<String>) -> Output {
    for note in notes {
        output.stderr.extend(format!("note: {note}\n").into_bytes());
    }
    output
}

/// `board schemas <command>`, as [`commands()`] reads `guide` and `notes`.
pub(crate) fn schemas(
    printer: Printer,
    guide: &Guide,
    command: &SchemasCommand,
    notes: Vec<String>,
) -> Output {
    let mut view = printer.view;
    match command {
        SchemasCommand::List(args) => {
            let listed = schemas::list(&mut view, guide, args);
            noted(Printer { view }.kit(SCHEMAS_LIST, listed), notes)
        }
        SchemasCommand::Show(args) => {
            let shown = schemas::show(&mut view, guide, args);
            noted(Printer { view }.kit(SCHEMAS_SHOW, shown), notes)
        }
    }
}
