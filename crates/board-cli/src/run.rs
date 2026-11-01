//! The command line and what each command does with it.

use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use board_model::{
    BINARY_NAME, CapabilityName, Code, DISPLAY_NAME, Fields, LogicalPath, Malformation, PathError,
    Refusal,
};
use board_ops::{Absent, Board, DEFAULT_LIMIT, Expect, MAX_LIMIT, Plugins, PutError};
use board_plugin::{Limits, Target};

/// What a plugin run gets from this command line's context.
pub(crate) fn plugins(context: &Context) -> Plugins<'_> {
    Plugins {
        directory: context.plugins.as_deref(),
        environment: context.environment.clone(),
        limits: Limits::default(),
        cache: context.cache.as_deref(),
    }
}
use clap::{CommandFactory, Parser, Subcommand};
use crumbkit::{Sink, Unparsed};

use crate::config::Context;
use crate::effects;
use crate::graft;
use crate::render::{self, GUIDE, Printer};
use crate::report::Output;
use crate::stdin::{TypeCommand, call, put};
use crate::timing::Ran;
use crate::values::{duration, expect, pointer, removal, var};

const ROOT_EXAMPLES: &str = "\
Start here:
  board init                        make this git repository a board
  board check                       check every entry; exit 1 if any is degraded
  board ls /                        list the board's root
  board get /projects/p             print the record in projects/p.json
  board put /tasks/a < task.json    write the record and commit it
  board query 'SELECT logical, status FROM entries LIMIT 5'

A path is logical and omits .json; without a leading / it is relative to
the current directory.
A record is a JSON object whose `$type`, such as runs/suite/v1, names its
schema at .board/types/runs/suite/v1.schema.json.

Tab completion: in fish, add `BOARD_COMPLETE=fish board | source` to
~/.config/fish/config.fish; in bash, add `eval \"$(BOARD_COMPLETE=bash board)\"`
to ~/.bashrc; in zsh, add `eval \"$(BOARD_COMPLETE=zsh board)\"` to ~/.zshrc.";

const QUERY_EXAMPLES: &str = "\
Tables and views:
  entries   one row per entry: source, path, logical, blob, size, kind, type,
            status, code, change, role, schema
  records   entries of kind record, with body, the record as JSON
  refs, links, validations, blobs, trees, worktree, mounts, sources,
  state

Rows stop at 64 MiB of output, even with --all; --json then sets `truncated`
and `cut.code`, board::output::over_budget, and the text notes it. A first
row past 64 MiB is refused with that code.

Examples:
  board query \"SELECT logical, code FROM entries WHERE status = 'degraded'\"
  board query \"SELECT logical FROM records WHERE type = 'tasks/task/v2'\" --limit 20
  board query 'SELECT name, sql FROM sqlite_master'";

#[derive(Debug, Parser)]
#[command(
    name = BINARY_NAME,
    version,
    about = format!("{DISPLAY_NAME}: a fast, validated view of a git tree."),
    after_help = ROOT_EXAMPLES,
)]
pub(crate) struct Cli {
    /// Run as if Board started in this directory, as `git -C` does.
    #[arg(short = 'C', value_name = "DIR", value_hint = clap::ValueHint::DirPath, value_parser = clap::builder::OsStringValueParser::new())]
    directories: Vec<OsString>,
    /// Run as the personal board, from any directory.
    ///
    /// `personal` in `$XDG_CONFIG_HOME/board/config.json` names the personal board's directory. The run starts at its root, and any -C is taken from there.
    #[arg(long)]
    personal: bool,
    #[command(flatten)]
    view: crumbkit::ViewFlags,
    #[command(subcommand)]
    command: Command,
}

/// How many entries a listing shows (STANDARDS.md 23).
pub(crate) type Page = crumbkit::Page<DEFAULT_LIMIT, MAX_LIMIT>;

pub(crate) use board_ops::Prefix;

/// A path argument as typed: absolute, or relative to the run's prefix.
#[derive(Clone, Debug)]
pub(crate) struct Given(String);

impl From<String> for Given {
    fn from(text: String) -> Self {
        Self(text)
    }
}

/// `put`'s arguments.
#[derive(Debug, clap::Args)]
pub(crate) struct Putting {
    /// Where to write: a logical path that leaves out .json, such as /tasks/a, or a from /tasks.
    #[arg(required_unless_present_any = ["batch", "new"])]
    pub(crate) path: Option<Given>,
    /// Write at a new name in the folder PATH names, which ends in /, or in the type's folder
    ///
    /// The name is the put's UTC minute and four random hex digits, such as
    /// 20261030T1405Z-c2e1. The put never overwrites a file, and its report names the path.
    /// With no PATH, the record's schema names the folder in `x-board-folder`.
    #[arg(long, conflicts_with_all = ["expect", "batch", "file"])]
    pub(crate) new: bool,
    /// Fill {name} in the type's folder; repeatable.
    ///
    /// The value fills one path segment, so it holds no /. A later --var of the same name wins.
    #[arg(long, value_name = "NAME=VALUE", requires = "new", conflicts_with_all = ["path", "batch"], value_parser = var)]
    pub(crate) var: Vec<(String, String)>,
    /// The blob the file must hold, or `none` for a new file.
    #[arg(long, value_parser = expect)]
    pub(crate) expect: Option<Expect>,
    /// Read JSON Lines of `{path, expect}` with a record, a text or `remove: true`; apply all or none.
    ///
    /// A line `{new, record}` writes the record in the folder `new` names, as --new does. A record line at `/.board/types/<namespace>/<kind>/v<n>.schema` adds that schema and takes
    /// no expect. Schema lines are checked before the other lines, and a schema the board holds
    /// is committed if HEAD lacks it, unless a symlink holds it. Every line writes in one board: this board, or the board one mount leads to. A
    /// batch through a mount commits in the mounted board and moves the mount's pin.
    #[arg(long, conflicts_with_all = ["path", "expect"])]
    pub(crate) batch: bool,
    /// Write stdin as a file at a path that is its whole name, such as /notes/plan.md.
    ///
    /// With a `store` setting, --file uploads bytes over 1 MiB or not UTF-8 under their digest and
    /// writes a board/attachment/v1 record.
    #[arg(long, conflicts_with = "batch")]
    pub(crate) file: bool,
    /// How long to wait for the board's lock, with a unit: 500ms, 30s, 2m or 1h.
    #[arg(long, value_name = "DURATION", value_parser = duration, default_value = "5s")]
    pub(crate) wait: Duration,
}

/// `refs`'s arguments.
#[derive(Debug, clap::Args)]
struct Refs {
    /// Each entry's logical path, such as /projects/p, or p from /projects; several share one check of the board.
    #[arg(required = true, value_name = "PATH", num_args = 1..=MAX_LIMIT)]
    paths: Vec<Given>,
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
        /// Check only the entries at or beneath this logical path, such as /projects, or . for the current directory.
        path: Option<Given>,
        #[command(flatten)]
        page: Page,
    },
    /// Print the file at each path, following pointers.
    #[command(after_help = render::GET_HELP)]
    Get {
        /// Each file's logical path, such as /projects/p, or p from /projects; a JSON file's leaves out .json.
        #[arg(required = true, value_name = "PATH", num_args = 1..=MAX_LIMIT)]
        paths: Vec<Given>,
        /// Fetch an attachment's bytes from the store and print them if their digest matches.
        #[arg(long)]
        fetch: bool,
    },
    /// List the directory at a path, without following pointers inside it.
    #[command(after_help = "\
Examples:
  board ls
  board ls /projects --limit 20")]
    Ls {
        /// The directory's logical path, such as /projects, or . for the current directory; the board's root when left out.
        path: Option<Given>,
        #[command(flatten)]
        page: Page,
    },
    /// List the references each entry makes and the entries that reach it.
    #[command(after_help = render::REFS_HELP)]
    Refs(Refs),
    /// Write the record on stdin at a path or --new name, or a file with --file.
    #[command(after_help = "\
The record must satisfy .board/types/<namespace>/<kind>/v<n>.schema.json, the
schema its `$type` names. Each put is committed unless .board/board.json sets
`commit_on_put` to false.

Examples:
  board put /tasks/a < task.json
  board put /tasks/a --expect none < task.json
  board put --new /tasks/ < task.json
  board put --new --var project=board < question.json
  board get /tasks/a | board put /tasks/b
  board put /notes/plan.md --file < plan.md
  board put --batch < writes.jsonl")]
    Put(Putting),
    /// Remove the file at a path.
    #[command(after_help = "\
The path names one file: a record, a file such as /notes/plan.md, or a link
or mount record itself. A path through a link removes the file it leads to.
-r removes every file beneath a directory in one commit, a link as itself,
and refuses a symlink. Check reports references to removed files. Each
removal is committed unless .board/board.json sets `commit_on_put` to false.

Examples:
  board rm /tasks/a
  board rm /tasks/a --expect 2bb218481e3ef5fe92e9fd97979a4578cab9af2c
  board rm -r /tasks
  board rm /tasks/zz --ignore-unmatch")]
    Rm {
        /// The file's logical path, such as /tasks/a, or a from /tasks; a JSON file's leaves out .json.
        path: Given,
        /// The blob the file must hold.
        #[arg(long, value_parser = expect)]
        expect: Option<Expect>,
        /// Remove every file beneath a directory.
        #[arg(short = 'r', long, conflicts_with = "expect")]
        recursive: bool,
        /// Report a path that names nothing as absent.
        #[arg(long)]
        ignore_unmatch: bool,
        /// How long to wait for the board's lock, with a unit: 500ms, 30s, 2m or 1h.
        #[arg(long, value_name = "DURATION", value_parser = duration, default_value = "5s")]
        wait: Duration,
    },
    /// Fetch the remote branch, replay this board's commits onto it, and push.
    #[command(after_help = "\
Board fetches the remote branch HEAD's branch follows, replays each commit
only this board has onto it, pushes the result, and moves HEAD, git's index
and the working tree to it. A commit that changed a file the remote also
changed is refused. A file edited during the sync keeps its edit and is
listed as kept. A fetch runs the `git` executable, so git's remotes,
credentials and SSH settings apply.

Examples:
  board sync
  board sync --no-push")]
    Sync {
        /// Fetch and replay, and leave the push for later.
        #[arg(long)]
        no_push: bool,
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
  board plugin find --capability available
  board plugin enable tasks"
    )]
    Plugin(crate::plugin::PluginCommand),
    /// Add types.
    #[command(
        subcommand,
        after_help = "Examples:\n  board type add notes/note/v1 < note.schema.json"
    )]
    Type(TypeCommand),
    /// Call a plugin's capability with flags, or else JSON on stdin.
    #[command(
        group = clap::ArgGroup::new("callee").required(true).args(["target", "capability"]),
        after_help = "\
Board reads stdin only when no flag is given and the capability admits an
input property that is not a flag; `-h` on the capability says which. With
--capability, the input goes on stdin. A plugin's alias runs a capability
too: with the alias gh, `board gh pr-create` is `board call github.pr-create`.

Examples:
  board call tasks.available
  board call tasks.add < input.json
  board call --capability available < input.json
  board call tasks.available -h")]
    Call {
        /// `<plugin>.<capability>`, such as tasks.available.
        target: Option<Target>,
        /// The capability of the one enabled plugin that offers it.
        #[arg(long)]
        capability: Option<CapabilityName>,
    },
    /// Ask the owner to admit a plugin's op.
    #[command(after_help = "\
Decide asks for the plugin BOARD_PLUGIN names. The owner's approval admits
the op, and their refusal refuses it with board::settings::refused. With no
answer, decide writes a pending effect in the personal board and refuses with
board::settings::asked; the owner answers it with board answer. The owner's
own op, with BOARD_PLUGIN unset, is admitted.

--text or --record shows the owner what the op writes; an approval admits
only those bytes.

Examples:
  board decide --op review.comment --path /reviews/r1
  board decide --op review.comment --path /reviews/r1 --text - < comment.md
  board decide --op review.comment --path /reviews/r1 --json")]
    Decide(effects::Deciding),
    /// Answer a pending effect.
    #[command(after_help = "\
`board --personal ls /effects/pending` lists the pending effects. An answer
moves one to /effects/answered/ as it writes the seen record. A plugin may
not answer.

An approval runs ~/.local/bin/attention-sign, which signs after Touch ID,
and checks it against ~/.config/board/allowed_signers, in the passwd home.
--signature skips the signer and checks the signature given.

Examples:
  board answer 4b2e0c8a9d1f37b6e5a0c2d84f9b1e7a3c6d0f8b2a5e9c1d4f7a0b3e6c9d2f5a approve
  board answer 4b2e0c8a9d1f37b6e5a0c2d84f9b1e7a3c6d0f8b2a5e9c1d4f7a0b3e6c9d2f5a refuse --reason 'not this week'
  board answer 4b2e0c8a9d1f37b6e5a0c2d84f9b1e7a3c6d0f8b2a5e9c1d4f7a0b3e6c9d2f5a approve --signature phone.sig")]
    Answer(effects::Answering),
    /// Run one read-only SQL statement over the index.
    #[command(after_help = QUERY_EXAMPLES)]
    Query {
        /// One read-only SQL statement, quoted as one argument.
        sql: String,
        #[command(flatten)]
        page: Page,
    },
    /// Serve this board's pages on 127.0.0.1 until it is idle.
    #[command(after_help = crate::ui::SERVE_HELP)]
    Serve {
        /// How long to wait for a request before exiting, with a unit: 500ms, 30s, 2m or 1h.
        #[arg(long, value_name = "DURATION", value_parser = duration, default_value = "30m")]
        idle: Duration,
    },
    /// List kinds, print a slot, or open a page.
    #[command(subcommand, after_help = crate::ui::UI_HELP)]
    Ui(crate::ui::UiCommand),
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

impl Command {
    /// Whether the command writes where the put check does not reach, so
    /// only the owner runs it.
    fn owner_only(&self) -> bool {
        matches!(
            self,
            Command::Init
                | Command::Type(_)
                | Command::Plugin(crate::plugin::PluginCommand::Enable { .. })
        )
    }
}

/// `none`, or a blob id in hex.
/// Runs one command line against the board found from `context`, reading a
/// record from `stdin` when the command takes one, and traces it in `ran`.
pub(crate) fn run(
    arguments: impl IntoIterator<Item = OsString>,
    context: &Context,
    stdin: &mut dyn Read,
    ran: &mut Ran,
) -> Output {
    let mut arguments: Vec<OsString> = arguments.into_iter().collect();
    if let Some(completed) = crate::complete::complete(&arguments, context) {
        return completed;
    }
    let parsing = Instant::now();
    let aliases = crate::alias::before(&mut arguments, context);
    let found = graft::find(&arguments, context.built_in, context.plugins.as_deref());
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let sink = &mut Sink::new(&mut stdout, &mut stderr);
    let parsed = crumbkit::try_parse_to::<graft::Parsed, _, _, _, _>(
        sink,
        graft::command(found.as_ref()).subcommands(aliases),
        &graft::guide(found.as_ref()),
        arguments.clone(),
    );
    ran.trace.add("parse", parsing);
    let (graft::Parsed { cli, grafted }, view) = match parsed {
        Ok(parsed) => parsed,
        Err(unparsed) => {
            if let Unparsed::Refused(refused) = &unparsed
                && let Some(output) = reread(&arguments, context, stdin, ran, refused)
            {
                return output;
            }
            let code = unparsed.print_to(sink).number();
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
    let printer = Printer { view };
    if cli.command.owner_only()
        && let Some(refusal) = effects::from_plugin(context, ran)
    {
        return render::refused(printer, *refusal);
    }
    let base = cli.personal.then(|| context.personal());
    let base = base.unwrap_or_else(|| Ok(context.start.clone()));
    let start = match base.and_then(|base| entered(&base, &cli.directories)) {
        Ok(start) => start,
        Err(refusal) => return render::refused(printer, *refusal),
    };
    if let Command::Init = cli.command {
        return match board_ops::init(&start) {
            Ok(report) => render::init(printer, &report),
            Err(error) => {
                let outside = matches!(error, board_ops::InitError::NotARepository { .. });
                render::refused(
                    printer,
                    board_ops::unopened(error.code(), &error, &Fields::default(), outside),
                )
            }
        };
    }
    if let Command::Answer(answering) = cli.command {
        return effects::answer(context, printer, answering, stdin);
    }
    let opening = Instant::now();
    let opened = open(context, &start);
    ran.trace.add("open", opening);
    let (mut board, prefix) = match opened {
        Ok(opened) => opened,
        Err(refusal) => return render::refused(printer, *refusal),
    };
    board.build_in(context.built_in.0);
    if let Err(refusal) = ask(&mut board, context, &cli.command) {
        return render::refused(printer, *refusal);
    }
    ran.root = Some(board.repository().path().to_path_buf());
    ran.prefix = prefix.as_ref().map(|prefix| match prefix {
        Ok(path) => path.to_string(),
        Err(PathError::Malformed { path, .. }) => path.clone(),
    });
    let prefix = prefix.as_ref();
    let output = match grafted {
        Some((target, mut flags)) => {
            graft::resolve(found.as_ref(), &mut flags, prefix);
            let stdin = graft::reads_stdin(found.as_ref()).then_some(stdin);
            call(&board, context, printer, &target, Some(flags), stdin)
        }
        None => match cli.command {
            Command::Decide(deciding) => effects::decide(&board, printer, prefix, &deciding, stdin),
            command => self::command(command, &board, context, printer, prefix, stdin),
        },
    };
    ran.trace.extend(board.take_trace());
    output
}

/// The output of a refused command line that Board reads again: one that
/// gives more paths than a run answers, or whose first word is an alias.
fn reread(
    arguments: &[OsString],
    context: &Context,
    stdin: &mut dyn Read,
    ran: &mut Ran,
    refused: &crumbkit::Refused,
) -> Option<Output> {
    if let Some(output) = render::too_many_paths(refused) {
        return Some(output);
    }
    Some(match crate::alias::after(arguments, context, refused)? {
        crate::alias::Resolved::Call(called) => run(called, context, stdin, ran),
        crate::alias::Resolved::Refused(refusal) => {
            let printer = Printer {
                view: refused.output().clone(),
            };
            render::refused(printer, *refusal)
        }
    })
}

/// Lets `board` ask the owner to admit what `command` writes. Only a
/// plugin's decide writes in the personal board.
fn ask(board: &mut Board, context: &Context, command: &Command) -> Result<(), Box<Refusal>> {
    let personal = match command {
        Command::Put(_) | Command::Rm { .. } => false,
        Command::Decide(_) => context.environment.plugin.is_some(),
        _ => return Ok(()),
    };
    board.ask_as(effects::asker(context, personal)?);
    Ok(())
}

/// Runs `command` against `board`, reading each path it is given from
/// `prefix`, or refuses a path that does not read as a logical path there.
fn command(
    command: Command,
    board: &Board,
    context: &Context,
    printer: Printer,
    prefix: Option<&Prefix>,
    stdin: &mut dyn Read,
) -> Output {
    let cache = context.cache.as_deref();
    let at = |given: &[Given], name| -> Result<Vec<_>, _> {
        given.iter().map(|path| read(prefix, path, name)).collect()
    };
    // A path that does not read is refused before anything else prints.
    macro_rules! or_refuse {
        ($read:expr) => {
            match $read {
                Ok(read) => read,
                Err(refusal) => return render::refused(printer, *refusal),
            }
        };
    }
    match command {
        // `run` takes these before this match: init and answer open no
        // board, and decide is matched there to keep this function short.
        Command::Init | Command::Answer(_) | Command::Decide(_) => unreachable!(),
        Command::Check { path, page } => {
            let path = or_refuse!(path.map(|path| read(prefix, &path, "check")).transpose());
            match board.check(path.as_ref(), cache) {
                Ok(report) => render::check(printer, &report, &page),
                Err(error) => render::refused(printer, error.refusal()),
            }
        }
        Command::Get { paths, fetch } => {
            let paths = or_refuse!(at(&paths, "get"));
            get(board, printer, &paths, fetch)
        }
        Command::Put(putting) => put(board, printer, prefix, putting, stdin),
        Command::Rm {
            path,
            expect,
            recursive,
            ignore_unmatch,
            wait,
            ..
        } => {
            let path = or_refuse!(read(prefix, &path, "rm"));
            let (removing, unmatched) = removal(recursive, expect, ignore_unmatch);
            match board.remove_waiting(&path, removing, unmatched, wait) {
                Ok(report) => render::put(printer, &report, None),
                Err(error) => render::refused(printer, error.refusal()),
            }
        }
        Command::Sync { no_push } => match board.sync(!no_push) {
            Ok(report) => render::sync(printer, &report),
            Err(error) => render::refused(printer, error.refusal()),
        },
        Command::Mount(MountCommand::Update { path }) => match board.mount_update(&path) {
            Ok(report) => render::mount_update(printer, &report),
            Err(error) => render::refused(printer, error.refusal()),
        },
        Command::Refs(asked) => {
            let paths = or_refuse!(at(&asked.paths, "refs"));
            refs(printer, board, context, &asked, &paths)
        }
        Command::Query { sql, page } => {
            let budget = Some(render::rows_budget(printer.view.budget()));
            let limit = page.limit().unwrap_or(usize::MAX);
            match board.query(&sql, limit, budget, cache) {
                Ok(report) => render::query(printer, &report),
                Err(error) => render::refused(printer, error.refusal()),
            }
        }
        Command::Type(command) => crate::stdin::type_add(command, board, printer, stdin),
        Command::Plugin(command) => crate::plugin::plugin(command, board, context, printer),
        Command::Call { target, capability } => {
            let target = match (target, capability) {
                (Some(target), _) => target,
                (None, Some(capability)) => Target::Capability(capability),
                (None, None) => unreachable!("clap requires a target or a capability"),
            };
            call(board, context, printer, &target, None, Some(stdin))
        }
        Command::Serve { idle } => crate::ui::serve(board, context, printer, idle),
        Command::Ui(command) => crate::ui::ui(command, board, context, printer, prefix),
        Command::Ls { path, page } => {
            let path =
                or_refuse!(path.map_or(Ok(LogicalPath::root()), |path| read(prefix, &path, "ls")));
            match board.ls(&path, cache) {
                Ok(report) => render::ls(printer, &report, &page),
                Err(error) => render::refused(printer, error.refusal("ls")),
            }
        }
    }
}

/// `given` read from `prefix`, which a relative path needs to be a logical
/// path, or a refusal naming `given` as typed. A refused `..` that stays at
/// or below `/` hints the absolute path it meant.
pub(crate) fn read(
    prefix: Option<&Prefix>,
    given: &Given,
    name: &str,
) -> Result<LogicalPath, Box<Refusal>> {
    let text = given.0.as_str();
    let from = match prefix {
        Some(prefix) if !text.starts_with('/') => prefix.clone(),
        _ => Ok(LogicalPath::root()),
    };
    let from = from.map_err(|error| Box::new(plain(error.code(), &error)))?;
    from.join(text)
        .map_err(|PathError::Malformed { reason, .. }| {
            let dots = reason == Malformation::DotSegment;
            let unsupported = dots.then_some(", which Board does not support");
            let unsupported = unsupported.unwrap_or_default();
            let message =
                format!("path {text:?}, read from {from}, is refused: {reason}{unsupported}");
            let meant = dots.then(|| climbed(&from, text)).flatten();
            let hint = meant.map_or_else(
                || format!("run `board {name} -h`"),
                |meant| format!("use {meant} for {text}"),
            );
            Box::new(plain(Code::CliUsage, &message).with_hint(hint))
        })
}

/// The folder `--new` names, read from `prefix` without its trailing `/`,
/// or a refusal when it has none.
pub(crate) fn read_folder(
    prefix: Option<&Prefix>,
    given: &Given,
) -> Result<LogicalPath, Box<Refusal>> {
    let text = board_ops::folder_text(&given.0);
    let folder = read(prefix, &Given(text.to_owned()), "put")?;
    if given.0.ends_with('/') {
        Ok(folder)
    } else {
        Err(Box::new(PutError::NotAFolder { folder }.refusal()))
    }
}

/// `text` read from `from` with each `.` and `..` resolved, for a hint only,
/// or `None` when it climbs above `/` or is malformed for another reason.
fn climbed(from: &LogicalPath, text: &str) -> Option<LogicalPath> {
    let mut segments: Vec<&str> = from.segments().collect();
    for segment in text.split('/') {
        match segment {
            "" | "." => {}
            ".." => drop(segments.pop()?),
            segment => segments.push(segment),
        }
    }
    format!("/{}", segments.join("/")).parse().ok()
}

/// A refusal with the hint for its code.
pub(crate) fn plain(code: Code, message: &dyn std::fmt::Display) -> Refusal {
    Refusal::new(code, message, &Fields::default())
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

/// `get` of `paths`, with each attachment's bytes when `fetch` is set.
fn get(board: &Board, printer: Printer, paths: &[LogicalPath], fetch: bool) -> Output {
    let answers = board.get_each(paths).map(|each| {
        each.map(move |answer| match answer {
            Ok(report) if fetch => board.fetched(report),
            answer => answer,
        })
    });
    render::get(printer, paths, answers)
}

/// `refs` of `paths`, read from the paths asked, with `--missing` or without.
fn refs(
    printer: Printer,
    board: &Board,
    context: &Context,
    asked: &Refs,
    paths: &[LogicalPath],
) -> Output {
    let absent = if asked.missing {
        Absent::Locate
    } else {
        Absent::Refuse
    };
    let limit = asked.page.limit();
    let cache = context.cache.as_deref();
    match board.refs(paths, absent, &asked.fields, limit, cache) {
        Err(error) => render::refused(printer, error.refusal()),
        Ok(answers) => render::refs(printer, paths, answers, limit),
    }
}

/// The board holding `start`, or else the one known board with a mount
/// whose repository holds `start`, and `start`'s logical path in that board,
/// opened as this build.
pub(crate) fn open(
    context: &Context,
    start: &Path,
) -> Result<(Board, Option<Prefix>), Box<Refusal>> {
    let (home, cache) = (context.home.as_deref(), context.cache.as_deref());
    let (mut board, prefix) = board_ops::open(start, home, cache, || context.known())?;
    board.built_by(crate::timing::DECIDER);
    Ok((board, prefix))
}
