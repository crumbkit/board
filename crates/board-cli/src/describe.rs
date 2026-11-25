//! `board commands list`, `board schemas list` and `board schemas show`,
//! which describe Board's own commands and the documents they print. They
//! run in any directory, and in a board they also describe the capabilities
//! of the plugins it enables.

use std::borrow::Cow;
use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;
use std::time::Instant;

use board_model::{Code, RecordType, Refusal, RefusalCode};
use board_ops::{BuiltIn, MountsAt, Trace};
use clap::{Args, Subcommand};
use crumbkit::commands::{self, COMMANDS_LIST, CommandsList};
use crumbkit::schemas::{self, SCHEMAS_LIST, SchemasList};
use crumbkit::{Batch, Call, Document, Failure, Recovery, Row, Sink, Tool, codes};
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::Serialize;
use serde_json::Value;

use crate::config::Context;
use crate::graft;
use crate::render::{Printer, TOOL};
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
    ///
    /// It also prints the schema of each record type a built-in plugin
    /// ships, as `board plugin enable` writes it, which `board schemas list`
    /// does not list.
    #[command(after_help = "\
Examples:
  board schemas show board/check-report/v2
  board schemas show rules/source/v1")]
    Show(SchemasShow),
}

/// The tool record and the parsed view of a listing command line `arguments`,
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
    view: crumbkit::Run,
    trace: &mut Trace,
) -> (Tool, Vec<String>, crumbkit::Run) {
    let opening = Instant::now();
    let opened = start.and_then(|start| context.open(&start, MountsAt::Pins));
    trace.add("open", opening);
    let mut board = match opened {
        Ok((board, _)) => board,
        Err(refusal) if refusal.code == RefusalCode::Board(Code::ConfigNotFound) => {
            return (TOOL, Vec::new(), view);
        }
        Err(refusal) => {
            let note = format!(
                "{}: the board cannot be opened, so no plugin capability is listed: {}",
                refusal.code.as_str(),
                refusal.message
            );
            return (TOOL, vec![note], view);
        }
    };
    board.build_in(context.built_in.0);
    let listing = Instant::now();
    let (grafted, notes) = graft::enabled(&board, context.plugins.as_deref());
    trace.add("plugin.list", listing);
    trace.count("capabilities", grafted.len());
    let tool = graft::tool(&grafted);
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let sink = &mut Sink::new(&mut stdout, &mut stderr);
    let command = graft::command(
        &grafted,
        <crate::run::Cli as clap::CommandFactory>::command(),
    );
    let parsed = crumbkit::try_parse_to::<graft::Parsed>(sink, command, &tool, arguments);
    // The line parsed against Board's own tree, so it parses with more leaves.
    let view = parsed.map_or(view, |(_, view)| view);
    (tool, notes, view)
}

/// `board commands <command>`, with the commands of `tool` and `notes`
/// on the plugins whose capabilities it leaves out.
pub(crate) fn commands(
    printer: Printer,
    tool: &Tool,
    command: &CommandsCommand,
    notes: Vec<String>,
) -> Output {
    let CommandsCommand::List(args) = command;
    let mut view = printer.view;
    let listed = commands::list(&mut view, tool, args);
    noted(Printer { view }.kit(COMMANDS_LIST, listed), notes)
}

/// `output` with each of `notes` on its own line of stderr.
fn noted(mut output: Output, notes: Vec<String>) -> Output {
    for note in notes {
        output.stderr.extend(format!("note: {note}\n").into_bytes());
    }
    output
}

/// `board schemas <command>`, as [`commands()`] reads `tool` and `notes`,
/// where `schemas show` also serves the record types `built_in` ships.
pub(crate) fn schemas(
    printer: Printer,
    tool: &Tool,
    command: &SchemasCommand,
    notes: Vec<String>,
    built_in: BuiltIn,
) -> Output {
    let mut view = printer.view;
    match command {
        SchemasCommand::List(args) => {
            let listed = schemas::list(&mut view, tool, args);
            noted(Printer { view }.kit(SCHEMAS_LIST, listed), notes)
        }
        SchemasCommand::Show(args) => {
            let shown = show(&mut view, tool, args, built_in);
            noted(Printer { view }.kit(SHOWN, Ok(shown)), notes)
        }
    }
}

/// The arguments of `board schemas show`, as crumbkit's `SchemasShow`
/// declares them, which keeps its ids to itself.
#[derive(Args, Clone, Debug, PartialEq, Eq)]
#[command(about = None, long_about = None)]
pub(crate) struct SchemasShow {
    /// The ids of the documents that `schemas list` lists, or of the record
    /// types the built-in plugins ship, separated by spaces
    #[arg(required = true, value_name = "ID", num_args = 1..=100)]
    ids: Vec<String>,
}

/// crumbkit's `SCHEMAS_SHOW`, which its leaf declares, with Board's row,
/// for printing; a test keeps the two schemas equal.
pub(crate) const SHOWN: Document<Batch<Shown, crumbkit::Error>> = Document::new(
    "crumbkit",
    "schemas-show",
    1,
    "The JSON Schema of each document id given.",
);

/// The path of `schemas show`, and of `schemas list`, which an unknown id's
/// recovery runs.
const SHOW_PATH: &[&str] = &["schemas", "show"];
const LIST_PATH: &[&str] = &["schemas", "list"];

/// The JSON Schema of each id in `args`, in order: a document Board
/// prints, as crumbkit serves it, or else a record type `built_in` ships,
/// as `plugin enable` writes it. crumbkit's `schemas::show` serves only the
/// former and builds rows no other crate can.
fn show(
    run: &mut crumbkit::Run,
    tool: &Tool,
    args: &SchemasShow,
    built_in: BuiltIn,
) -> Batch<Shown, crumbkit::Error> {
    let served = schemas::documents(run.command(), tool);
    let mut shipped: Option<Vec<(RecordType, Value)>> = None;
    let mut batch = Batch::new(SHOW_PATH, "ids", &args.ids);
    for id in &args.ids {
        let document = served.iter().find(|document| document.id() == *id);
        let schema = document.map(|document| document.schema().to_value());
        let schema = schema.or_else(|| {
            let record_type = id.parse::<RecordType>().ok()?;
            let shipped = shipped.get_or_insert_with(|| {
                (built_in.0.iter())
                    .flat_map(|plugin| plugin.schemas())
                    .collect()
            });
            (shipped.iter())
                .find(|(shipped, _)| *shipped == record_type)
                .map(|(_, schema)| schema.clone())
        });
        batch.push(match schema {
            Some(schema) => Ok(Shown {
                id: id.clone(),
                schema,
            }),
            None => Err(NotFound { id: id.clone() }.into()),
        });
    }
    batch
}

/// The JSON Schema of one id, as crumbkit's `SchemaShowRow` prints it.
#[derive(Serialize)]
pub(crate) struct Shown {
    id: String,
    schema: Value,
}

impl Row for Shown {
    const COLUMNS: &'static [&'static str] = &["id", "schema"];
    // A schema cut short is not JSON, so a narrow table never cuts it.
    const UNCUT_COLUMNS: &'static [&'static str] = &["schema"];
}

/// crumbkit's `SchemaShowRow` schema, word for word, so
/// `board/schemas-show/v1` stays as released.
impl JsonSchema for Shown {
    fn schema_name() -> Cow<'static, str> {
        "SchemaShowRow".into()
    }

    fn schema_id() -> Cow<'static, str> {
        "crumbkit::schemas::SchemaShowRow".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "The JSON Schema of one document that the tool prints.",
            "type": "object",
            "properties": {
                "id": {
                    "description": "The document's id, such as `shelf/books-list/v1`.",
                    "type": "string",
                },
                "schema": {
                    "description": "The document's 2020-12 JSON Schema.",
                    "type": ["object", "boolean"],
                },
            },
            "required": ["id", "schema"],
        })
    }
}

/// An id that names no document and no built-in record type, which
/// crumbkit's own `schemas show` refuses in the same words.
#[derive(Debug)]
struct NotFound {
    id: String,
}

impl fmt::Display for NotFound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "board has no document {}", self.id)
    }
}

impl std::error::Error for NotFound {}

impl Failure for NotFound {
    fn code(&self) -> crumbkit::Code<'_> {
        codes::SCHEMAS_NOT_FOUND
    }

    fn recovery(&self) -> Recovery {
        Recovery::Run(Call::new(LIST_PATH))
    }
}
