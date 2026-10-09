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
use clap_complete::env::Shells;
use clap_complete::{ArgValueCompleter, CompletionCandidate};

use crate::config::Context;
use crate::render::{self, Format};
use crate::report::Output;
use crate::run::{Cli, entered};

const VARIABLE: &str = "BOARD_COMPLETE";

/// What `BOARD_COMPLETE` asks of `arguments`: the registration script when
/// they are `board` alone, the candidates when they are `board -- <words>`,
/// and otherwise `None`, so the command runs. Empty or `0` asks nothing.
pub(crate) fn complete(arguments: &[OsString], context: &Context) -> Option<Output> {
    let shell = context
        .complete
        .as_deref()
        .filter(|shell| !shell.is_empty() && *shell != "0")?;
    let words = match arguments {
        [_] => None,
        [_, dashes, words @ ..] if dashes == "--" && !words.is_empty() => Some(words),
        _ => return None,
    };
    let shells = Shells::builtins();
    let Some(completer) = shell.to_str().and_then(|name| shells.completer(name)) else {
        return Some(match words {
            Some(_) => Output::default(),
            None => unknown(shell, &shells),
        });
    };
    let mut stdout = Vec::new();
    let written = match words {
        None => completer.write_registration(
            VARIABLE,
            BINARY_NAME,
            BINARY_NAME,
            BINARY_NAME,
            &mut stdout,
        ),
        Some(words) => {
            let start = start(words, completer.name(), context);
            let mut command = with_paths(
                Cli::command(),
                &paths(start.clone(), completer.name(), context),
            );
            completer.write_complete(&mut command, words.to_vec(), start.as_deref(), &mut stdout)
        }
    };
    if written.is_err() {
        stdout.clear();
    }
    Some(Output {
        stdout,
        ..Output::default()
    })
}

/// The refusal of a script for a shell completion does not support.
fn unknown(shell: &OsStr, shells: &Shells<'_>) -> Output {
    let names: Vec<&str> = shells.names().collect();
    render::refused(
        Format::Text,
        &Refusal::new(
            Code::UsageInvalid,
            format!(
                "{VARIABLE} names the shell '{}', which completion does not support",
                shell.to_string_lossy()
            ),
            &Fields::default(),
        )
        .with_hint(format!(
            "set {VARIABLE} to one of {}, as in `{VARIABLE}=fish {BINARY_NAME} | source`",
            names.join(", ")
        )),
    )
}

/// Where the command line being completed starts: after each `-C` before
/// the word under the cursor, or `None` if one cannot be entered. bash and
/// zsh send the whole line and the cursor's index; fish sends the line up
/// to the cursor, with every word but the last already unquoted.
fn start(words: &[OsString], shell: &str, context: &Context) -> Option<PathBuf> {
    let cursor = context
        .cursor
        .filter(|_| shell != "fish")
        .unwrap_or(words.len().saturating_sub(1));
    let before = words.get(1..cursor).unwrap_or_default();
    let mut directories = Vec::new();
    let mut words = before.iter();
    while let Some(word) = words.next() {
        if word == "--" {
            break;
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
    entered(&context.start, &directories, Format::Text).ok()
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
            if arg.get_value_parser().type_id() == TypeId::of::<LogicalPath>() {
                arg.add(completer.clone())
            } else {
                arg
            }
        })
        .mut_subcommands(|command| with_paths(command, completer))
}
