//! `board serve` and `board ui`: what each takes from the command line and
//! the context, and which report it prints.

use std::time::Duration;

use board_model::{Code, ComponentKind, Refusal};
use board_ops::Board;

use crate::config::Context;
use crate::render::{self, Printer};
use crate::report::Output;
use crate::run::{Given, Page, Prefix, plain, read};

pub(crate) const SERVE_HELP: &str = "\
It listens on 127.0.0.1 at a port the system picks, and writes the port and a
token made for this launch to a file in the board's cache. Every request must
name 127.0.0.1:<port> as its host, and every API request must carry the token.
It exits on SIGINT, SIGTERM or SIGHUP, or when no API request arrives for
--idle, and prints nothing until then. `board ui open` starts it when it is
not running.

Examples:
  board serve
  board serve --idle 2m";

pub(crate) const UI_HELP: &str = "\
A ui/view/v1 record names slots and their kinds. A ui/binding/v1 record
fills each slot with `ref`, a record's path, or `data`.

Examples:
  board ui kinds
  board ui template markdown
  board ui open /bindings/review";

#[derive(Debug, clap::Subcommand)]
pub(crate) enum UiCommand {
    /// List the component kinds a view's slot can name, with each kind's schema.
    #[command(after_help = "\
Examples:
  board ui kinds
  board ui kinds --long
  board ui kinds --long --json")]
    Kinds {
        #[command(flatten)]
        page: Page,
    },
    /// Print a slot's fill for a kind, to edit into a binding's `slots`.
    #[command(after_help = "\
Examples:
  board ui template markdown
  board ui template record --json")]
    Template {
        /// The component kind, as `board ui kinds` lists it.
        #[arg(value_parser = kind)]
        kind: ComponentKind,
    },
    /// Open a binding's page in a browser, starting `board serve` if it is not running.
    #[command(after_help = "\
The server writes an opener page, readable only by you, to the board's cache,
and `board ui open` opens it with $BROWSER when it is set, else with `open` on
macOS and `xdg-open` elsewhere. $BROWSER names one program, which gets the
opener's URL as its only argument. The opener gives the page a launch code, which
the page exchanges once for the server's token within a minute.

Examples:
  board ui open /bindings/review")]
    Open {
        /// The binding's logical path, such as /bindings/review.
        path: Given,
    },
}

/// A component kind by name.
fn kind(text: &str) -> Result<ComponentKind, String> {
    ComponentKind::parse(text).ok_or_else(|| {
        let names: Vec<&str> = ComponentKind::ALL.iter().map(|kind| kind.name()).collect();
        format!("must be one of: {}", names.join(", "))
    })
}

/// The cache directory, which the server file needs.
fn cache(context: &Context) -> Result<&std::path::Path, Box<Refusal>> {
    context.cache.as_deref().ok_or_else(|| {
        Box::new(plain(
            Code::ServeFailed,
            &"no cache directory is known: set XDG_CACHE_HOME or HOME",
        ))
    })
}

/// `board serve`: serves until idle or stopped by SIGINT, SIGTERM or SIGHUP, then
/// reports what it served.
pub(crate) fn serve(board: &Board, context: &Context, printer: Printer, idle: Duration) -> Output {
    let served = cache(context).and_then(|cache| {
        let stop = board_plugin::stop_on_signals().map_err(|error| {
            Box::new(plain(
                Code::ServeFailed,
                &format!("SIGINT, SIGTERM and SIGHUP cannot be caught: {error}"),
            ))
        })?;
        board_serve::serve(board, cache, idle, &stop)
            .map_err(|error| Box::new(plain(error.code(), &error)))
    });
    match served {
        Ok(served) => render::serve(printer, &served),
        Err(refusal) => render::refused(printer, *refusal),
    }
}

/// `board ui <command>`, reading a path argument from `prefix`.
pub(crate) fn ui(
    command: UiCommand,
    board: &Board,
    context: &Context,
    printer: Printer,
    prefix: Option<&Prefix>,
) -> Output {
    match command {
        UiCommand::Kinds { page } => render::ui_kinds(printer, &page),
        UiCommand::Template { kind } => render::ui_template(printer, kind),
        UiCommand::Open { path } => {
            let path = match read(prefix, &path, "ui") {
                Ok(path) => path,
                Err(refusal) => return render::refused(printer, *refusal),
            };
            let opened = cache(context).and_then(|cache| {
                let launch = board_serve::Launch {
                    cache: cache.to_path_buf(),
                    binary: context.binary.clone().unwrap_or_else(|| "board".into()),
                    browser: context.browser.clone(),
                };
                board_serve::open(board, &launch, &path).map_err(|error| {
                    Box::new(match error {
                        board_serve::OpenError::Page(error) => error.refusal(),
                        error @ board_serve::OpenError::Failed { .. } => {
                            plain(error.code(), &error)
                        }
                        board_serve::OpenError::Refused { ref hint, .. } => {
                            let refusal = plain(error.code(), &error);
                            Refusal {
                                hint: Some(hint.clone())
                                    .filter(|hint| !hint.is_empty())
                                    .or(refusal.hint.clone()),
                                ..refusal
                            }
                        }
                    })
                })
            });
            match opened {
                Ok(opened) => render::ui_open(printer, &opened),
                Err(refusal) => render::refused(printer, *refusal),
            }
        }
    }
}
