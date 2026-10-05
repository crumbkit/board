//! The command line and what each command does with it.

use std::ffi::OsString;
use std::io::Read;

use board_model::{BINARY_NAME, Code, DISPLAY_NAME, LogicalPath, MAX_RECORD_BYTES, ObjectId};
use board_ops::{Board, DEFAULT_LIMIT, Expect, MAX_LIMIT, Plugins};
use board_plugin::{CapabilityName, Limits, MetaFilter, PluginName, Target};

/// What a plugin run gets from this command line's context.
fn plugins(context: &Context) -> Plugins<'_> {
    Plugins {
        directory: context.plugins.as_deref(),
        environment: context.environment.clone(),
        limits: Limits::default(),
    }
}
use clap::error::{ContextKind, ContextValue, ErrorKind};
use clap::{CommandFactory, Parser, Subcommand};

use crate::config::Context;
use crate::render::{self, Format, hint};
use crate::report::Output;

const ROOT_EXAMPLES: &str = "\
Start here:
  board init                   make this git repository a board
  board check                  check every entry; exit 1 if any is degraded
  board ls /                   list the board's root
  board get /projects/p        print the record in projects/p.json
  board query 'SELECT logical, status FROM entries LIMIT 5'

A path is logical: it starts with / and leaves out .json.
A record is a JSON object whose `$type`, such as lr/task/v1, names its schema,
here .board/types/lr/task/v1.schema.json.
Run `board <command> --help` for a command's arguments and examples.";

const QUERY_EXAMPLES: &str = "\
Tables and views:
  entries   one row per entry: source, path, logical, blob, size, kind, type,
            status, code, change, role, schema
  records   entries of kind record, with body, the record as JSON
  refs, validations, blobs, trees, worktree, mounts, sources, extracted, state
  p_<name>  each enabled plugin's database

Examples:
  board query \"SELECT logical, code FROM entries WHERE status = 'degraded'\"
  board query \"SELECT logical FROM records WHERE type = 'lr/task/v1'\" --limit 20
  board query 'SELECT name, sql FROM sqlite_master'";

#[derive(Debug, Parser)]
#[command(
    name = BINARY_NAME,
    version,
    about = format!("{DISPLAY_NAME}: a fast, validated view of a git tree."),
    after_help = ROOT_EXAMPLES,
)]
pub(crate) struct Cli {
    /// Print one JSON document instead of text.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

/// How many entries a listing shows (STANDARDS.md 23).
#[derive(Debug, Clone, Copy, clap::Args)]
struct Page {
    /// The most entries to show, from 1 to 10000.
    #[arg(long, default_value_t = DEFAULT_LIMIT, value_parser = limit, conflicts_with = "all")]
    limit: usize,
    /// Show every entry.
    #[arg(long)]
    all: bool,
}

impl Page {
    fn limit(self) -> Option<usize> {
        (!self.all).then_some(self.limit)
    }
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Make the git repository here a board by writing a minimal .board/board.json.
    #[command(after_help = "\
An existing .board/board.json is left as it is. Nothing is committed.

Examples:
  board init
  board init --json")]
    Init,
    /// Check every entry, or those at or beneath a path; fail if any is degraded.
    #[command(after_help = "\
Degraded entries come first, so a limited listing never hides one.

Examples:
  board check
  board check /projects --all
  board check --json")]
    Check {
        /// Check only the entries at or beneath this logical path, such as /projects.
        path: Option<LogicalPath>,
        #[command(flatten)]
        page: Page,
    },
    /// Print the file at a path, following pointers; with --json, only a record.
    #[command(after_help = "\
Examples:
  board get /projects/p
  board get /projects/p --json
  board get /README.md")]
    Get {
        /// The file's logical path: it starts with /, and a JSON file's leaves out .json, such as /projects/p.
        path: LogicalPath,
    },
    /// List the directory at a path, without following pointers inside it.
    #[command(after_help = "\
Examples:
  board ls
  board ls /projects --limit 20")]
    Ls {
        /// The directory's logical path; the board's root when left out.
        path: Option<LogicalPath>,
        #[command(flatten)]
        page: Page,
    },
    /// List the references a record makes and the records that reach it.
    #[command(after_help = "\
Lines are `from <pointer> <target> <status>` for each reference the record
makes, then `to <path> <pointer>` for each record that reaches it.

Examples:
  board refs /projects/p
  board refs /projects/p --json")]
    Refs {
        /// The record's logical path, such as /projects/p.
        path: LogicalPath,
        #[command(flatten)]
        page: Page,
    },
    /// Check the record on stdin and write it, in canonical form, at a path.
    #[command(after_help = "\
The record must satisfy .board/types/<namespace>/<kind>/v<n>.schema.json, the
schema its `$type` names. Each put is committed unless .board/board.json sets
`commit_on_put` to false.

Examples:
  board put /tasks/a < task.json
  board put /tasks/a --expect none < task.json
  board get /tasks/a | board put /tasks/b
  board put --batch < writes.jsonl")]
    Put {
        /// Where to write: a logical path that leaves out .json, such as /tasks/a.
        #[arg(required_unless_present = "batch")]
        path: Option<LogicalPath>,
        /// The blob the file must hold, or `none` for a new file.
        #[arg(long, value_parser = expect)]
        expect: Option<Expect>,
        /// Read JSON Lines of `{path, expect, record}` and write them all or none.
        #[arg(long, conflicts_with_all = ["path", "expect"])]
        batch: bool,
    },
    /// Work with mounts.
    #[command(subcommand)]
    Mount(MountCommand),
    /// Work with plugins.
    #[command(subcommand)]
    Plugin(PluginCommand),
    /// Call a plugin's capability with the input JSON on stdin.
    #[command(
        group = clap::ArgGroup::new("callee").required(true).args(["target", "capability"]),
        after_help = "\
Examples:
  board call tasks.due < input.json
  board call --capability due < input.json")]
    Call {
        /// `<plugin>.<capability>`, such as tasks.due.
        target: Option<Target>,
        /// The capability of the one enabled plugin that offers it.
        #[arg(long)]
        capability: Option<CapabilityName>,
    },
    /// Run one read-only SQL statement over the index.
    #[command(after_help = QUERY_EXAMPLES)]
    Query {
        /// One read-only SQL statement, quoted as one argument.
        sql: String,
        /// The most rows to return, from 1 to 10000.
        #[arg(long, default_value_t = DEFAULT_LIMIT, value_parser = limit)]
        limit: usize,
    },
}

#[derive(Debug, Subcommand)]
enum MountCommand {
    /// Advance the mount at a path, which is not followed, to its `follow` ref.
    #[command(after_help = "\
Examples:
  board mount update /vendor/lib")]
    Update {
        /// The mount's logical path, such as /vendor/lib.
        path: LogicalPath,
    },
}

#[derive(Debug, Subcommand)]
enum PluginCommand {
    /// List the installed plugins, their capabilities and whether this board enables them.
    #[command(after_help = "\
Examples:
  board plugin list
  board plugin list --json")]
    List {
        #[command(flatten)]
        page: Page,
    },
    /// List the installed plugins' capabilities that match every filter.
    #[command(
        group = clap::ArgGroup::new("filter").required(true).multiple(true).args(["capability", "filters"]),
        after_help = "\
Examples:
  board plugin find --capability due
  board plugin find --meta category=tasks --json")]
    Find {
        /// Only capabilities of this name.
        #[arg(long)]
        capability: Option<CapabilityName>,
        /// `<key>=<value>`: the capability's metadata, or else its plugin's, holds this string.
        #[arg(long = "meta")]
        filters: Vec<MetaFilter>,
        #[command(flatten)]
        page: Page,
    },
    /// Enable an installed plugin: write the schemas it ships, then name it in .board/board.json.
    #[command(after_help = "\
Examples:
  board plugin enable tasks
  board plugin enable tasks --json")]
    Enable {
        /// The installed plugin's name, as `board plugin list` shows it.
        name: PluginName,
    },
}

/// `none`, or a blob id in hex.
fn expect(text: &str) -> Result<Expect, String> {
    if text == "none" {
        return Ok(Expect::Absent);
    }
    ObjectId::from_hex(text)
        .map(Expect::Blob)
        .ok_or_else(|| "must be `none` or a blob id in lowercase hex".to_owned())
}

/// A row limit from 1 to [`MAX_LIMIT`].
fn limit(text: &str) -> Result<usize, String> {
    match text.parse::<usize>() {
        Ok(limit) if (1..=MAX_LIMIT).contains(&limit) => Ok(limit),
        _ => Err(format!("must be a whole number from 1 to {MAX_LIMIT}")),
    }
}

/// Runs one command line against the board found from `context`, reading a
/// record from `stdin` when the command takes one.
pub(crate) fn run(
    arguments: impl IntoIterator<Item = OsString>,
    context: &Context,
    stdin: &mut dyn Read,
) -> Output {
    let arguments: Vec<OsString> = arguments.into_iter().collect();
    let cli = match Cli::try_parse_from(&arguments) {
        Ok(cli) => cli,
        Err(error) => return refused(&error, &arguments),
    };
    let format = if cli.json { Format::Json } else { Format::Text };
    if let Command::Init = cli.command {
        return match board_ops::init(&context.start) {
            Ok(report) => render::init(format, &report),
            Err(error) => render::error(format, error.code(), &error),
        };
    }
    let board = match Board::open(&context.start, context.home.clone()) {
        Ok(board) => board,
        Err(error) => return render::error(format, error.code(), &error),
    };
    match cli.command {
        Command::Init => unreachable!("init runs before a board is opened"),
        Command::Check { path, page } => match board.check(path.as_ref(), context.cache.as_deref())
        {
            Ok(report) => render::check(format, &report, page.limit()),
            Err(error) => render::error(format, error.code(), &error),
        },
        Command::Get { path } => match board.get(&path) {
            Ok(report) => render::get(format, &report),
            Err(error) => failed_at(format, "get", &path, error.code(), &error),
        },
        Command::Put { batch: true, .. } => {
            match board.put_batch(&mut std::io::BufReader::new(stdin)) {
                Ok(report) => render::put(format, &report),
                Err(error) => render::put_error(format, &error),
            }
        }
        Command::Put {
            path: Some(path),
            expect,
            ..
        } => {
            let record = match bounded(stdin) {
                Ok(record) => record,
                Err(error) => {
                    return render::error(
                        format,
                        Code::RepoUnreadable,
                        &format!("stdin cannot be read: {error}"),
                    );
                }
            };
            match board.put(&path, expect.unwrap_or(Expect::Any), &record) {
                Ok(report) => render::put(format, &report),
                Err(error) => render::put_error(format, &error),
            }
        }
        Command::Put { path: None, .. } => unreachable!("clap requires a path without --batch"),
        Command::Mount(MountCommand::Update { path }) => match board.mount_update(&path) {
            Ok(report) => render::mount_update(format, &report),
            Err(error) => render::error(format, error.code(), &error),
        },
        Command::Refs { path, page } => match board.refs(&path, context.cache.as_deref()) {
            Ok(report) => render::refs(format, &report, page.limit()),
            Err(error) => failed_at(format, "refs", &path, error.code(), &error),
        },
        Command::Query { sql, limit } => {
            match board.query(&sql, limit, context.cache.as_deref(), &plugins(context)) {
                Ok(report) => render::query(format, &report),
                Err(error) => render::query_error(format, &error),
            }
        }
        Command::Plugin(PluginCommand::Enable { name }) => {
            match board.plugin_enable(context.plugins.as_deref(), &name) {
                Ok(report) => render::plugin_enable(format, &report),
                Err(error) => render::error(format, error.code(), &error),
            }
        }
        Command::Plugin(PluginCommand::List { page }) => {
            match board.plugin_list(context.plugins.as_deref()) {
                Ok(report) => render::plugin_list(format, &report, page.limit()),
                Err(error) => render::error(format, error.code(), &error),
            }
        }
        Command::Plugin(PluginCommand::Find {
            capability,
            filters,
            page,
        }) => match board.plugin_find(context.plugins.as_deref(), capability.as_ref(), &filters) {
            Ok(report) => render::plugin_find(format, &report, page.limit()),
            Err(error) => render::error(format, error.code(), &error),
        },
        Command::Call { target, capability } => {
            call(&board, context, format, target, capability, stdin)
        }
        Command::Ls { path, page } => {
            let path = path.unwrap_or_else(LogicalPath::root);
            match board.ls(&path) {
                Ok(report) => render::ls(format, &report, page.limit()),
                Err(error) => failed_at(format, "ls", &path, error.code(), &error),
            }
        }
    }
}

/// A failure reading `path`. When nothing is there and the path ends in
/// `.json`, the hint names the path without it, since a logical path leaves
/// the extension out.
fn failed_at(
    format: Format,
    command: &str,
    path: &LogicalPath,
    code: Code,
    error: &dyn std::fmt::Display,
) -> Output {
    match path.as_str().strip_suffix(".json") {
        Some(stem) if code == Code::PathNotFound => render::failed(
            format,
            code.as_str(),
            &error.to_string(),
            Some(&format!(
                "a logical path leaves out .json: try `{BINARY_NAME} {command} {stem}`"
            )),
            serde_json::Map::new(),
        ),
        _ => render::error(format, code, error),
    }
}

/// A command line clap refused, as an error with code `usage::invalid` and
/// exit code 2; help, asked for, is printed to stdout with exit code 0. The
/// message and hint come from clap's error context, never its rendered text.
fn refused(error: &clap::Error, arguments: &[OsString]) -> Output {
    if matches!(
        error.kind(),
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
    ) {
        return Output {
            stdout: error.render().to_string().into_bytes(),
            ..Output::default()
        };
    }
    let command = command_path(arguments);
    let named = if command.is_empty() {
        BINARY_NAME.to_owned()
    } else {
        format!("{BINARY_NAME} {command}")
    };
    let help = if let Some(corrected) = corrected(error, arguments) {
        format!("a logical path starts with / and leaves out .json: try `{corrected}`")
    } else if command.is_empty() {
        hint(Code::UsageInvalid).to_owned()
    } else {
        format!("run `{named} --help` for its usage and examples")
    };
    let mut hints = tips(error);
    hints.push(help);
    let hint = hints.join("; ");
    let wants_json = arguments
        .iter()
        .skip(1)
        .take_while(|argument| *argument != "--")
        .any(|argument| argument == "--json");
    let format = if wants_json {
        Format::Json
    } else {
        Format::Text
    };
    let mut output = render::failed(
        format,
        Code::UsageInvalid.as_str(),
        &message(error, &named),
        Some(&hint),
        serde_json::Map::new(),
    );
    // A group named without its command prints its help before the hint.
    if format == Format::Text && error.kind() == ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
    {
        output.stderr = format!(
            "error: {}: {}\n\n{}\nhint: {hint}\n",
            Code::UsageInvalid,
            message(error, &named),
            error.render().to_string().trim_end()
        )
        .into_bytes();
    }
    output.code = 2;
    output
}

/// One piece of clap's context for `error`, as text.
fn context(error: &clap::Error, kind: ContextKind) -> Option<String> {
    Some(match error.get(kind)? {
        ContextValue::String(text) => text.clone(),
        ContextValue::Strings(texts) => texts.join("', '"),
        ContextValue::StyledStrs(texts) => texts
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; "),
        _ => return None,
    })
}

/// What clap refused, naming the value at fault; `named` is the command
/// that was run, such as `board plugin`.
fn message(error: &clap::Error, named: &str) -> String {
    let get = |kind| context(error, kind).unwrap_or_default();
    let argument = get(ContextKind::InvalidArg);
    let value = get(ContextKind::InvalidValue);
    match error.kind() {
        ErrorKind::InvalidSubcommand => {
            format!(
                "unrecognized subcommand '{}'",
                get(ContextKind::InvalidSubcommand)
            )
        }
        ErrorKind::UnknownArgument => format!("unexpected argument '{argument}' found"),
        // Board offers no fixed set of values, so clap refuses a value it
        // was given only through the parser, as `ValueValidation`.
        ErrorKind::InvalidValue => {
            format!("a value is required for '{argument}' but none was supplied")
        }
        ErrorKind::ValueValidation => {
            let why = std::error::Error::source(error)
                .map_or_else(String::new, |source| format!(": {source}"));
            format!("invalid value '{value}' for '{argument}'{why}")
        }
        ErrorKind::ArgumentConflict => format!(
            "the argument '{argument}' cannot be used with '{}'",
            get(ContextKind::PriorArg)
        ),
        ErrorKind::MissingRequiredArgument => format!(
            "the following required arguments were not provided: {}",
            argument.replace("', '", ", ")
        ),
        ErrorKind::MissingSubcommand | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand => {
            format!("{named} needs a command")
        }
        kind => kind.as_str().map_or_else(
            || "the command line is refused".to_owned(),
            str::to_lowercase,
        ),
    }
}

/// clap's suggestions for `error`: a similar command, argument or value, or
/// the values it accepts.
fn tips(error: &clap::Error) -> Vec<String> {
    [
        (
            ContextKind::SuggestedSubcommand,
            "a similar subcommand exists: '",
        ),
        (ContextKind::SuggestedArg, "a similar argument exists: '"),
        (ContextKind::SuggestedValue, "a similar value exists: '"),
        (ContextKind::ValidValue, "possible values: '"),
    ]
    .into_iter()
    .filter_map(|(kind, lead)| {
        context(error, kind)
            .filter(|found| !found.is_empty())
            .map(|found| format!("{lead}{found}'"))
    })
    .chain(context(error, ContextKind::Suggested).filter(|tip| !tip.is_empty()))
    .collect()
}

/// The command line with the value clap refused written as a logical path,
/// such as `/tasks/a` for `tasks/a.json`, when clap admits it that way.
fn corrected(error: &clap::Error, arguments: &[OsString]) -> Option<String> {
    let Some(ContextValue::String(value)) = error.get(ContextKind::InvalidValue) else {
        return None;
    };
    let stem = value.strip_suffix(".json").unwrap_or(value);
    let path = format!("/{}", stem.trim_start_matches('/'));
    let at = arguments
        .iter()
        .skip(1)
        .position(|given| given == value.as_str())?
        + 1;
    let mut fixed = arguments.to_vec();
    fixed[at] = path.into();
    Cli::try_parse_from(&fixed).ok()?;
    let words: Vec<String> = fixed[1..]
        .iter()
        .map(|word| {
            let word = word.to_string_lossy();
            if word
                .chars()
                .all(|c| c.is_alphanumeric() || "/._-=".contains(c))
            {
                word.into_owned()
            } else {
                format!("'{}'", word.replace('\'', r"'\''"))
            }
        })
        .collect();
    Some(format!("{BINARY_NAME} {}", words.join(" ")))
}

/// The subcommands named at the start of `arguments`, such as `plugin find`,
/// so a refusal can point at that command's help.
fn command_path(arguments: &[OsString]) -> String {
    let mut command = Cli::command();
    let mut path = Vec::new();
    for argument in arguments
        .iter()
        .skip(1)
        .filter_map(|argument| argument.to_str())
    {
        if argument.starts_with('-') {
            continue;
        }
        match command.find_subcommand(argument) {
            Some(found) => {
                path.push(argument.to_owned());
                command = found.clone();
            }
            None => break,
        }
    }
    path.join(" ")
}

/// `call`: the target, or the capability alone, with the input on stdin.
fn call(
    board: &Board,
    context: &Context,
    format: Format,
    target: Option<Target>,
    capability: Option<CapabilityName>,
    stdin: &mut dyn Read,
) -> Output {
    let target = match (target, capability) {
        (Some(target), _) => target,
        (None, Some(capability)) => Target::Capability(capability),
        (None, None) => unreachable!("clap requires a target or a capability"),
    };
    let input = match bounded(stdin) {
        Ok(input) => input,
        Err(error) => {
            return render::error(
                format,
                Code::PluginBadInput,
                &format!("stdin cannot be read: {error}"),
            );
        }
    };
    match board.call(&target, &input, &plugins(context), context.cache.as_deref()) {
        Ok(report) => render::call(format, &report),
        Err(error) => render::call_error(format, &error),
    }
}

/// stdin, read one byte past the record bound so a parse sees input that is
/// too large.
fn bounded(stdin: &mut dyn Read) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    stdin
        .take(MAX_RECORD_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}
