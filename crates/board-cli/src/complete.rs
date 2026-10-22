//! Shell completion (docs/architecture.md, Commands). `BOARD_COMPLETE=<shell>
//! board` prints a script that registers completion; the script then runs
//! `BOARD_COMPLETE=<shell> board -- <words>` for each Tab, and this prints
//! the candidates. `clap_complete`'s dynamic engine completes commands and
//! flags, and every logical path argument asks the board for its names.
//! Completion never writes, and never prints an error into the shell.

use std::any::TypeId;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use board_model::{BINARY_NAME, Code, Fields, LogicalPath, Refusal};
use board_ops::Board;
use clap::{Command, CommandFactory};
use clap_complete::{ArgValueCompleter, CompletionCandidate};
use crumbkit::Sink;

use crate::config::Context;
use crate::render::{self, GUIDE, Printer};
use crate::report::Output;
use crate::run::{Cli, Given, entered};

const VARIABLE: &str = "BOARD_COMPLETE";

/// What `BOARD_COMPLETE` asks of `arguments`: the registration script when
/// they are `board` alone, the candidates when they are `board -- <words>`,
/// and otherwise `None`, so the command runs. Empty or `0` asks nothing.
pub(crate) fn complete(arguments: &[OsString], context: &Context) -> Option<Output> {
    let shell = context
        .complete
        .as_deref()
        .filter(|shell| !shell.is_empty() && *shell != "0")?;
    let script = match arguments {
        [_] => true,
        [_, dashes, words @ ..] if dashes == "--" && !words.is_empty() => false,
        _ => return None,
    };
    let stem = Path::new(shell).file_stem().unwrap_or(shell);
    let Some(name) = SHELLS.into_iter().find(|name| stem == *name) else {
        return Some(if script {
            unknown(shell)
        } else {
            Output::default()
        });
    };
    // The script runs `board` from PATH, however this run was started.
    let arguments = std::iter::once(BINARY_NAME.into()).chain(arguments[1..].iter().cloned());
    let command = |typed: &[OsString]| {
        let start = start(typed, name, context);
        with_paths(Cli::command(), &paths(start, name, context))
    };
    let mut stdout = Vec::new();
    let sink = &mut Sink::new(&mut stdout, std::io::sink());
    // A line crumbkit cannot complete prints nothing, which is all the
    // shell needs; its exit code goes unread.
    let _ = crumbkit::complete_to(sink, command, &GUIDE, VARIABLE, shell, arguments, None);
    Some(Output {
        stdout,
        ..Output::default()
    })
}

/// The shells completion supports.
const SHELLS: [&str; 5] = ["bash", "elvish", "fish", "powershell", "zsh"];

/// The refusal of a script for a shell completion does not support.
fn unknown(shell: &OsStr) -> Output {
    let message = format!(
        "{VARIABLE} names the shell '{}', which completion does not support",
        shell.to_string_lossy()
    );
    let hint = format!(
        "set {VARIABLE} to one of {}, as in `{VARIABLE}=fish {BINARY_NAME} | source`",
        SHELLS.join(", ")
    );
    let refusal = Refusal::new(Code::CliUsage, message, &Fields::default()).with_hint(hint);
    let printer = Printer::new(false, &GUIDE, [BINARY_NAME]);
    render::refused(printer, refusal)
}

/// Where the command line being completed starts: after `--personal` and
/// each `-C` before the word under the cursor among the words `typed` after
/// `board`, or `None` if one cannot be entered. bash and zsh send the whole
/// line and the cursor's index; fish sends the line up to the cursor, with
/// every word but the last already unquoted.
fn start(words: &[OsString], shell: &str, context: &Context) -> Option<PathBuf> {
    let cursor = context
        .cursor
        .filter(|_| shell != "fish")
        .unwrap_or(words.len());
    let before = words.get(..cursor.saturating_sub(1)).unwrap_or_default();
    let (mut base, mut directories) = (context.start.clone(), Vec::new());
    let mut words = before.iter();
    while let Some(word) = words.next() {
        if word == "--" {
            break;
        }
        if word == "--personal" {
            base = crate::known::personal(context).ok()?;
        }
        let directory = if word == "-C" {
            words.next().and_then(|word| word.to_str())
        } else {
            word.to_str()
                .and_then(|word| word.strip_prefix("-C"))
                .map(|directory| directory.strip_prefix('=').unwrap_or(directory))
        };
        if let Some(directory) = directory {
            let directory = if shell == "fish" {
                directory.to_owned()
            } else {
                unescape(directory)
            };
            directories.push(home(&directory, context)?);
        }
    }
    entered(&base, &directories).ok()
}

/// `directory` with a leading `~` read as the home directory, as a shell
/// would have expanded it; `None` when it needs a home and there is none.
fn home(directory: &str, context: &Context) -> Option<OsString> {
    match directory.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => Some(
            context
                .home
                .as_ref()?
                .join(rest.trim_start_matches('/'))
                .into_os_string(),
        ),
        _ => Some(directory.into()),
    }
}

/// The word under the cursor as the shell will pass it once the line runs:
/// backslash escapes and single or double quotes removed.
pub(crate) fn unescape(word: &str) -> String {
    let mut unescaped = String::with_capacity(word.len());
    let mut quote = None;
    let mut characters = word.chars();
    while let Some(character) = characters.next() {
        match (quote, character) {
            (None | Some('"'), '\\') => unescaped.extend(characters.next()),
            (None, '\'' | '"') => quote = Some(character),
            (Some(open), _) if open == character => quote = None,
            _ => unescaped.push(character),
        }
    }
    unescaped
}

/// `path` as `shell` should be offered it. bash inserts a candidate as it
/// is, so each character it would read specially is escaped with `\`.
/// Inside quotes the `\` stays literal, so bash cannot complete there, and
/// bash 4 and later send the word without its quote, so Board cannot tell.
/// fish and zsh quote what they insert themselves.
pub(crate) fn escaped(path: &str, shell: &str) -> String {
    if shell != "bash" {
        return path.to_owned();
    }
    let mut escaped = String::with_capacity(path.len());
    for character in path.chars() {
        if !(character.is_alphanumeric() || "/._-+=:@,%".contains(character)) {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

/// Completes a logical path, for `shell`, from the board found from
/// `start`.
fn paths(start: Option<PathBuf>, shell: &'static str, context: &Context) -> ArgValueCompleter {
    let home = context.home.clone();
    ArgValueCompleter::new(move |current: &OsStr| {
        let board = start
            .as_deref()
            .and_then(|start: &Path| Board::open(start, home.clone()).ok());
        match (board, current.to_str()) {
            (Some(board), Some(typed)) => board
                .complete(&unescape(typed))
                .into_iter()
                .map(|path| CompletionCandidate::new(escaped(&path, shell)))
                .collect(),
            _ => Vec::new(),
        }
    })
}

/// `command` with `completer` on every argument that takes a logical path,
/// in it and every subcommand.
fn with_paths(command: Command, completer: &ArgValueCompleter) -> Command {
    command
        .mut_args(|arg| {
            let id = arg.get_value_parser().type_id();
            if id == TypeId::of::<LogicalPath>() || id == TypeId::of::<Given>() {
                arg.add(completer.clone())
            } else {
                arg
            }
        })
        .mut_subcommands(|command| with_paths(command, completer))
}
