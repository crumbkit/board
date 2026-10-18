//! The command line and what each command does with it.

use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Instant;

use board_model::{
    BINARY_NAME, Code, DISPLAY_NAME, Fields, LogicalPath, MAX_RECORD_BYTES, ObjectId, Refusal,
};
use board_ops::{Absent, Board, DEFAULT_LIMIT, Expect, MAX_LIMIT, Plugins};
use board_plugin::{CapabilityName, Limits, MetaFilter, PluginName, Target};
use serde_json::{Map, Value};

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
use crumbkit::Sink;

use crate::config::Context;
use crate::graft;
use crate::render::{self, GUIDE, Printer};
use crate::report::Output;
use crate::timing::Ran;

const ROOT_EXAMPLES: &str = "\
Start here:
  board init                        make this git repository a board
  board check                       check every entry; exit 1 if any is degraded
  board ls /                        list the board's root
  board get /projects/p             print the record in projects/p.json
  board put /tasks/a < task.json    write the record and commit it
  board query 'SELECT logical, status FROM entries LIMIT 5'

A path is logical: it starts with / and leaves out .json.
A record is a JSON object whose `$type`, such as tasks/task/v1, names its
schema, here .board/types/tasks/task/v1.schema.json.
Run `board <command> --help` for a command's arguments and examples.

Tab completion of commands, flags and board paths: in fish, add
`BOARD_COMPLETE=fish board | source` to ~/.config/fish/config.fish; in bash,
add `eval \"$(BOARD_COMPLETE=bash board)\"` to ~/.bashrc; in zsh, add
`eval \"$(BOARD_COMPLETE=zsh board)\"` to ~/.zshrc.";

const QUERY_EXAMPLES: &str = "\
Tables and views:
  entries   one row per entry: source, path, logical, blob, size, kind, type,
            status, code, change, role, schema
  records   entries of kind record, with body, the record as JSON
  refs, validations, blobs, trees, worktree, mounts, sources, extracted, state
  p_<name>  each enabled plugin's database

Rows stop at 64 MiB of output, even with --all; --json then sets `truncated`
and `cut.code`, board::output::over_budget, and the text notes it. A first
row past 64 MiB is refused with that code.

Examples:
  board query \"SELECT logical, code FROM entries WHERE status = 'degraded'\"
  board query \"SELECT logical FROM records WHERE type = 'tasks/task/v1'\" --limit 20
  board query 'SELECT name, sql FROM sqlite_master'";

#[derive(Debug, Parser)]
#[command(
    name = BINARY_NAME,
    version,
    about = format!("{DISPLAY_NAME}: a fast, validated view of a git tree."),
    after_help = ROOT_EXAMPLES,
)]
pub(crate) struct Cli {
    /// Run as if Board were started in this directory, as `git -C` does.
    #[arg(short = 'C', value_name = "DIR", value_hint = clap::ValueHint::DirPath, value_parser = clap::builder::OsStringValueParser::new())]
    directories: Vec<OsString>,
    #[command(flatten)]
    view: crumbkit::ViewFlags,
    #[command(subcommand)]
    command: Command,
}

/// How many entries a listing shows (STANDARDS.md 23).
pub(crate) type Page = crumbkit::Page<DEFAULT_LIMIT, MAX_LIMIT>;

/// `refs`'s arguments.
#[derive(Debug, clap::Args)]
struct Refs {
    /// Each record's logical path, such as /projects/p; several are answered from one check of the board.
    #[arg(required = true, value_name = "PATH", num_args = 1..=MAX_LIMIT)]
    paths: Vec<LogicalPath>,
    /// When nothing is at a path yet, report where a file there would be instead of refusing.
    #[arg(long)]
    missing: bool,
    /// A JSON pointer, such as /status, whose value in each reference's target to show; can be repeated.
    #[arg(long = "field", value_name = "POINTER", value_parser = pointer)]
    fields: Vec<String>,
    #[command(flatten)]
    page: Page,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Make this git repository a board.
    #[command(after_help = "\
It writes a minimal .board/board.json, or leaves an existing one as it is.
Nothing is committed.

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
    /// Print the file at each path, following pointers.
    #[command(after_help = render::GET_HELP)]
    Get {
        /// Each file's logical path: it starts with /, and a JSON file's leaves out .json, such as /projects/p.
        #[arg(required = true, value_name = "PATH", num_args = 1..=MAX_LIMIT)]
        paths: Vec<LogicalPath>,
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
    /// List the references each record makes and the records that reach it.
    #[command(after_help = render::REFS_HELP)]
    Refs(Refs),
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
    #[command(
        subcommand,
        after_help = "\
A mount is a record of `$type` board/mount/v1 that holds `repo`, `follow` and
`commit`. Board reads the mounted repository at `commit`; `mount update`
advances `commit` to the commit `follow` names.

Examples:
  board mount update /vendor/lib"
    )]
    Mount(MountCommand),
    /// Work with plugins.
    #[command(
        subcommand,
        after_help = "\
Examples:
  board plugin list
  board plugin find --capability due
  board plugin enable tasks"
    )]
    Plugin(PluginCommand),
    /// Call a plugin's capability with flags, or else JSON on stdin.
    #[command(
        group = clap::ArgGroup::new("callee").required(true).args(["target", "capability"]),
        after_help = "\
Board reads stdin only when no flag is given.

Examples:
  board call tasks.due < input.json
  board call --capability due < input.json
  board call tasks.due -h")]
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
        #[command(flatten)]
        page: Page,
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
  board plugin list --long
  board plugin list --json")]
    List {
        #[command(flatten)]
        page: Page,
    },
    /// List the installed plugins' capabilities, or those that match every filter.
    #[command(after_help = "\
Examples:
  board plugin find
  board plugin find --capability due
  board plugin find --capability due --long
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

/// A JSON pointer, by RFC 6901: empty, or each token after a `/`, in which
/// `~` is followed by `0` or `1`.
fn pointer(text: &str) -> Result<String, String> {
    let mut escapes = text.split('~').skip(1);
    if (text.is_empty() || text.starts_with('/'))
        && escapes.all(|after| after.starts_with(['0', '1']))
    {
        Ok(text.to_owned())
    } else {
        Err("must be a JSON pointer, such as /status, or /a~1b for the key a/b: empty or starting with /, with ~ only as ~0 or ~1".to_owned())
    }
}

/// Runs one command line against the board found from `context`, reading a
/// record from `stdin` when the command takes one, and traces it in `ran`.
pub(crate) fn run(
    arguments: impl IntoIterator<Item = OsString>,
    context: &Context,
    stdin: &mut dyn Read,
    ran: &mut Ran,
) -> Output {
    let arguments: Vec<OsString> = arguments.into_iter().collect();
    if let Some(completed) = crate::complete::complete(&arguments, context) {
        return completed;
    }
    let parsing = Instant::now();
    let grafted = graft::find(&arguments, context.plugins.as_deref());
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let parsed = crumbkit::parse_to::<graft::Parsed, _, _, _, _>(
        &mut Sink::new(&mut stdout, &mut stderr),
        graft::command(grafted.as_ref()),
        &graft::guide(grafted.as_ref()),
        arguments.clone(),
    );
    ran.trace.add("parse", parsing);
    let (graft::Parsed { cli, grafted }, view) = match parsed {
        Ok(parsed) => parsed,
        Err(stop) => {
            let code = stop.number();
            if let Some(output) = (code == 2).then(|| split(&arguments)).flatten() {
                return output;
            }
            let error = (code == 2).then(|| Code::CliUsage.as_str().to_owned());
            return Output {
                stdout,
                stderr,
                code,
                error,
            };
        }
    };
    let words = arguments.get(1..).unwrap_or_default();
    ran.command = Some(crumbkit::command_path(&Cli::command(), &GUIDE, words).join(" "));
    let printer = &mut Printer { view };
    let start = match entered(&context.start, &cli.directories) {
        Ok(start) => start,
        Err(refusal) => return render::refused(printer, *refusal),
    };
    if let Command::Init = cli.command {
        return match board_ops::init(&start) {
            Ok(report) => render::init(printer, &report),
            Err(error) => {
                let outside = matches!(error, board_ops::InitError::NotARepository { .. });
                render::refused(printer, unopened(error.code(), &error, outside))
            }
        };
    }
    let opening = Instant::now();
    let opened = Board::open(&start, context.home.clone());
    ran.trace.add("open", opening);
    let board = match opened {
        Ok(board) => board,
        Err(error) => {
            let outside = matches!(error, board_ops::OpenError::NoRepository { .. });
            return render::refused(printer, unopened(error.code(), &error, outside));
        }
    };
    ran.root = Some(board.repository().path().to_path_buf());
    let output = match grafted {
        Some((target, flags)) => call(&board, context, printer, &target, Some(flags), stdin),
        None => command(cli.command, &board, context, printer, stdin),
    };
    ran.trace.extend(board.take_trace());
    output
}

/// Runs `command` against `board`.
fn command(
    command: Command,
    board: &Board,
    context: &Context,
    printer: &mut Printer,
    stdin: &mut dyn Read,
) -> Output {
    let cache = context.cache.as_deref();
    match command {
        Command::Init => unreachable!("init runs before a board is opened"),
        Command::Check { path, page } => match board.check(path.as_ref(), cache) {
            Ok(report) => render::check(printer, &report, &page),
            Err(error) => render::refused(printer, error.refusal()),
        },
        Command::Get { paths } => render::get(printer, &paths, board.get_each(&paths)),
        Command::Put { batch: true, .. } => {
            match board.put_batch(&mut std::io::BufReader::new(stdin)) {
                Ok(report) => render::put(printer, &report),
                Err(error) => render::refused(printer, error.refusal()),
            }
        }
        Command::Put {
            path: Some(path),
            expect,
            ..
        } => put(board, printer, &path, expect, stdin),
        Command::Put { path: None, .. } => unreachable!("clap requires a path without --batch"),
        Command::Mount(MountCommand::Update { path }) => match board.mount_update(&path) {
            Ok(report) => render::mount_update(printer, &report),
            Err(error) => render::refused(printer, error.refusal()),
        },
        Command::Refs(asked) => refs(printer, board, context, &asked),
        Command::Query { sql, page } => {
            let budget = Some(render::rows_budget(printer.view.budget()));
            let limit = page.limit().unwrap_or(usize::MAX);
            match board.query(&sql, limit, budget, cache, &plugins(context)) {
                Ok(report) => render::query(printer, &report),
                Err(error) => render::refused(printer, error.refusal()),
            }
        }
        Command::Plugin(PluginCommand::Enable { name }) => {
            match board.plugin_enable(context.plugins.as_deref(), &name) {
                Ok(report) => render::plugin_enable(printer, &report),
                Err(error) => render::refused(printer, plain(error.code(), &error)),
            }
        }
        Command::Plugin(PluginCommand::List { page }) => {
            match board.plugin_list(context.plugins.as_deref()) {
                Ok(report) => render::plugin_list(printer, &report, &page),
                Err(error) => render::refused(printer, plain(error.code(), &error)),
            }
        }
        Command::Plugin(PluginCommand::Find {
            capability,
            filters,
            page,
        }) => match board.plugin_find(context.plugins.as_deref(), capability.as_ref(), &filters) {
            Ok(report) => render::plugin_find(printer, &report, &page),
            Err(error) => render::refused(printer, plain(error.code(), &error)),
        },
        Command::Call { target, capability } => {
            let target = match (target, capability) {
                (Some(target), _) => target,
                (None, Some(capability)) => Target::Capability(capability),
                (None, None) => unreachable!("clap requires a target or a capability"),
            };
            call(board, context, printer, &target, None, stdin)
        }
        Command::Ls { path, page } => {
            let path = path.unwrap_or_else(LogicalPath::root);
            match board.ls(&path, cache) {
                Ok(report) => render::ls(printer, &report, &page),
                Err(error) => render::refused(printer, error.refusal("ls")),
            }
        }
    }
}

/// A refusal with the hint for its code.
fn plain(code: Code, message: &dyn std::fmt::Display) -> Refusal {
    Refusal::new(code, message, &Fields::default())
}

/// Glue for a usage failure crumbkit words alike for every tool: a run
/// given too many paths says to split them.
fn split(arguments: &[OsString]) -> Option<Output> {
    let error = Cli::try_parse_from(arguments).err()?;
    let (Some(ContextValue::String(argument)), Some(ContextValue::String(value))) = (
        error.get(ContextKind::InvalidArg),
        error.get(ContextKind::InvalidValue),
    ) else {
        return None;
    };
    if error.kind() != ErrorKind::TooManyValues {
        return None;
    }
    let (mut command, mut names) = (Cli::command(), vec![BINARY_NAME.to_owned()]);
    for word in arguments.iter().skip(1) {
        if let Some(sub) = command.find_subcommand(word).cloned() {
            names.push(sub.get_name().to_owned());
            command = sub;
        }
    }
    let message = format!(
        "{} was given more than {MAX_LIMIT} paths, and one run answers at most {MAX_LIMIT}",
        names[1..].join(" ")
    );
    let hint = format!("split the paths over several runs of at most {MAX_LIMIT} each");
    let mut refusal = plain(Code::CliUsage, &message).with_hint(hint);
    let name = argument.split(' ').next().unwrap_or(argument);
    refusal.fields.insert("argument".to_owned(), name.into());
    refusal
        .fields
        .insert("value".to_owned(), value.as_str().into());
    let json = (arguments.iter().skip(1))
        .take_while(|word| *word != "--")
        .any(|word| word == "--json");
    let flags = crumbkit::ViewFlags::new().json(json);
    let printer = &mut Printer {
        view: crumbkit::Output::new(flags, Cli::command(), &GUIDE, arguments.iter()),
    };
    Some(render::refused(printer, refusal))
}

/// `start` after entering each of `directories` in turn, as `git -C` does,
/// or the refusal of the first that is not a directory.
pub(crate) fn entered(start: &Path, directories: &[OsString]) -> Result<PathBuf, Box<Refusal>> {
    let mut start = start.to_path_buf();
    for directory in directories {
        let joined = start.join(directory);
        let fault = match std::fs::canonicalize(&joined) {
            Ok(path) if path.is_dir() => {
                start = path;
                continue;
            }
            Ok(_) => "is not a directory".to_owned(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                "does not exist".to_owned()
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotADirectory => {
                "is not a directory".to_owned()
            }
            Err(error) => format!("cannot be read: {error}"),
        };
        let message = format!(
            "cannot change to '{}': {} {fault}",
            Path::new(directory).display(),
            joined.display()
        );
        let hint = "give -C a directory that exists, such as the board's root or a directory \
                    inside it; a relative -C is taken from the one before it";
        return Err(Box::new(plain(Code::CliUsage, &message).with_hint(hint)));
    }
    Ok(start)
}

/// `refs` of one path or several, with `--missing` or without.
fn refs(printer: &mut Printer, board: &Board, context: &Context, asked: &Refs) -> Output {
    let absent = if asked.missing {
        Absent::Locate
    } else {
        Absent::Refuse
    };
    let limit = asked.page.limit();
    let cache = context.cache.as_deref();
    match board.refs(&asked.paths, absent, &asked.fields, limit, cache) {
        Err(error) => render::refused(printer, error.refusal()),
        Ok(answers) => render::refs(printer, &asked.paths, answers, limit),
    }
}

/// The refusal when a board cannot be opened or made. `outside` a git
/// repository, `board init` fails too, so the hint names only what can run.
fn unopened(code: Code, error: &dyn std::fmt::Display, outside: bool) -> Refusal {
    let refusal = Refusal::new(code, error, &Fields::default());
    if outside {
        refusal.with_hint(
            "run `git init` first to make the directory a repository; for a board elsewhere, run `board -C <dir> check`",
        )
    } else {
        refusal
    }
}

/// `put` of one record, read from stdin.
fn put(
    board: &Board,
    printer: &mut Printer,
    path: &LogicalPath,
    expect: Option<Expect>,
    stdin: &mut dyn Read,
) -> Output {
    let record = match bounded(stdin) {
        Ok(record) => record,
        Err(error) => {
            let message = format!("stdin cannot be read: {error}");
            return render::refused(printer, plain(Code::RepoUnreadable, &message));
        }
    };
    match board.put(path, expect.unwrap_or(Expect::Any), &record) {
        Ok(report) => render::put(printer, &report),
        Err(error) => render::refused(printer, error.refusal()),
    }
}

/// `call`: the target, or the capability alone, with the input as flags or,
/// when no flag is given, on stdin. A terminal on stdin, or nothing read
/// from it, is no input.
fn call(
    board: &Board,
    context: &Context,
    printer: &mut Printer,
    target: &Target,
    flags: Option<Map<String, Value>>,
    stdin: &mut dyn Read,
) -> Output {
    let read = match flags.filter(|flags| !flags.is_empty()) {
        Some(flags) => Ok(Value::Object(flags).to_string().into_bytes()),
        None if context.terminal => Ok(Vec::new()),
        None => bounded(stdin),
    };
    let input = match read {
        Ok(input) if input.trim_ascii().is_empty() => b"{}".to_vec(),
        Ok(input) => input,
        Err(error) => {
            let message = format!("stdin cannot be read: {error}");
            return render::refused(printer, plain(Code::PluginBadInput, &message));
        }
    };
    match board.call(target, &input, &plugins(context), context.cache.as_deref()) {
        Ok(report) => render::call(printer, &report),
        Err(error) => render::refused(printer, error.refusal()),
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
