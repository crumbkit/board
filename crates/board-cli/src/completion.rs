//! `board completion install`, `check` and `print`, which crumbkit's
//! `completion` module serves. They register the `BOARD_COMPLETE`
//! completion that `complete` answers (docs/architecture.md, Commands).

use clap::{CommandFactory as _, Subcommand};
use crumbkit::completion::{
    COMPLETION_CHECK, COMPLETION_INSTALL, COMPLETION_PRINT, CompletionCheck, CompletionInstall,
    CompletionPrint,
};
use crumbkit::help::{Effects, Example, Leaf};

use crate::render::Printer;
use crate::report::Output;
use crate::run::Cli;

/// `board completion -h`'s examples.
pub(crate) const HELP: &str = "\
Examples:
  board completion install --shell zsh
  board completion check --shell zsh
  board completion print --shell fish";

#[derive(Debug, Subcommand)]
pub(crate) enum CompletionCommand {
    /// Install Board's completion where the shell loads it.
    #[command(after_help = "\
Examples:
  board completion install --shell zsh
  board completion install --shell zsh --force")]
    Install(CompletionInstall),
    /// Check that the installed completion file matches this Board.
    #[command(after_help = "\
Examples:
  board completion check --shell zsh")]
    Check(CompletionCheck),
    /// Print Board's completion registration script for a shell.
    #[command(after_help = "\
Examples:
  board completion print --shell fish")]
    Print(CompletionPrint),
}

/// `board completion <command>`.
pub(crate) fn completion(command: &CompletionCommand, printer: Printer) -> Output {
    match command {
        CompletionCommand::Install(args) => printer.kit(
            COMPLETION_INSTALL,
            crumbkit::completion::install(Cli::command, args),
        ),
        CompletionCommand::Check(args) => printer.kit(
            COMPLETION_CHECK,
            crumbkit::completion::check(Cli::command, args),
        ),
        CompletionCommand::Print(args) => printer.kit(
            COMPLETION_PRINT,
            crumbkit::completion::print(Cli::command, args),
        ),
    }
}

pub(crate) const INSTALL: Leaf = Leaf::new(&["completion", "install"])
    .examples(&[
        Example::new(&["--shell", "fish"], "Install Board's completion for fish"),
        Example::new(
            &["--shell", "zsh", "--force"],
            "Install it for zsh, replacing a file or a link that install did not write",
        ),
    ])
    .effects(Effects::additive().idempotent())
    .document(COMPLETION_INSTALL);

pub(crate) const CHECK: Leaf = Leaf::new(&["completion", "check"])
    .examples(&[Example::new(
        &["--shell", "zsh"],
        "Report zsh's completion file when it is missing or differs",
    )])
    .effects(Effects::read_only())
    .document(COMPLETION_CHECK);

pub(crate) const PRINT: Leaf = Leaf::new(&["completion", "print"])
    .examples(&[Example::new(
        &["--shell", "fish"],
        "Print the registration script for fish",
    )])
    .effects(Effects::read_only())
    .document(COMPLETION_PRINT);
