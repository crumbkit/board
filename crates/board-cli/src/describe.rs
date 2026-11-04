//! `board commands list`, `board schemas list` and `board schemas show`,
//! which describe Board's own commands and the documents they print. They
//! open no board, so they run in any directory.

use clap::Subcommand;
use crumbkit::commands::{self, COMMANDS_LIST, CommandsList};
use crumbkit::schemas::{self, SCHEMAS_LIST, SCHEMAS_SHOW, SchemasList, SchemasShow};

use crate::render::{GUIDE, Printer};
use crate::report::Output;

/// The examples of `board commands`.
pub(crate) const COMMANDS_HELP: &str = "\
`board commands list` reads no board, so it runs in any directory. It
describes `call` but not the capabilities of each plugin; `board plugin find`
lists those.

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

/// `board commands <command>`.
pub(crate) fn commands(printer: Printer, command: &CommandsCommand) -> Output {
    let CommandsCommand::List(args) = command;
    let mut view = printer.view;
    let listed = commands::list(&mut view, &GUIDE, args);
    Printer { view }.kit(COMMANDS_LIST, listed)
}

/// `board schemas <command>`.
pub(crate) fn schemas(printer: Printer, command: &SchemasCommand) -> Output {
    let mut view = printer.view;
    match command {
        SchemasCommand::List(args) => {
            let listed = schemas::list(&mut view, &GUIDE, args);
            Printer { view }.kit(SCHEMAS_LIST, listed)
        }
        SchemasCommand::Show(args) => {
            let shown = schemas::show(&mut view, &GUIDE, args);
            Printer { view }.kit(SCHEMAS_SHOW, shown)
        }
    }
}
