//! Command lines that agents often guess, read as the one command they
//! mean (docs/plugins/calling/commands.md, Guessed words): a first word
//! such as `cat`, a long flag spelled with hyphens for underscores or the
//! reverse, and `--path` given to a built-in command whose path is
//! positional. Board reads them only after clap refused the line, so a line
//! that parses never reaches them, and only where one command fits.

use std::ffi::OsString;

use clap::error::{ContextKind, ContextValue, ErrorKind};
use clap::{Command, CommandFactory as _};

use crate::run::Cli;

/// Each first word that is no command, and the built-in command it means.
/// An enabled plugin's name or alias wins, because Board reads those first.
const WORDS: &[(&str, &str)] = &[
    ("cat", "get"),
    ("show", "get"),
    ("list", "ls"),
    ("schema", "schemas"),
];

/// The command line `arguments` mean, which clap `refused`, or `None` when
/// no reading fits and clap's refusal stands. Each reading ends: a word
/// becomes a built-in command, a flag takes a spelling clap names as one
/// of the command's flags, and a `--path` word is dropped.
pub(crate) fn meant(arguments: &[OsString], refused: &crumbkit::Refused) -> Option<Vec<OsString>> {
    let error = refused.error()?;
    match error.kind() {
        ErrorKind::InvalidSubcommand if refused.path().is_empty() => word(arguments, error),
        ErrorKind::UnknownArgument => {
            let ContextValue::String(given) = error.get(ContextKind::InvalidArg)? else {
                return None;
            };
            let at = flag_at(arguments, given)?;
            spelled(arguments, error, given, at).or_else(|| path(arguments, refused, at))
        }
        _ => None,
    }
}

/// `arguments` with a first word in [`WORDS`] replaced by the command it
/// means.
fn word(arguments: &[OsString], error: &clap::Error) -> Option<Vec<OsString>> {
    let Some(ContextValue::String(word)) = error.get(ContextKind::InvalidSubcommand) else {
        return None;
    };
    let (_, command) = WORDS.iter().find(|(guess, _)| guess == word)?;
    let (at, _) = crate::alias::first_word(arguments)?;
    let mut line = arguments.to_vec();
    *line.get_mut(at).filter(|given| *given == word.as_str())? = OsString::from(command);
    Some(line)
}

/// Where the flag `given` sits in `arguments`, as itself or as
/// `<given>=<value>`, before any `--`, after which every word is a value.
fn flag_at(arguments: &[OsString], given: &str) -> Option<usize> {
    let given = given.split_once('=').map_or(given, |(flag, _)| flag);
    let mut words = arguments.iter().take_while(|word| *word != "--");
    words.position(|word| {
        word.to_str().is_some_and(|word| {
            word == given
                || word
                    .strip_prefix(given)
                    .is_some_and(|rest| rest.starts_with('='))
        })
    })
}

/// `arguments` with the long flag at `at` spelled as the flag clap
/// suggested, when that differs from it only by hyphens for underscores or
/// the reverse.
fn spelled(
    arguments: &[OsString],
    error: &clap::Error,
    given: &str,
    at: usize,
) -> Option<Vec<OsString>> {
    let Some(ContextValue::String(suggested)) = error.get(ContextKind::SuggestedArg) else {
        return None;
    };
    let flag = given.split_once('=').map_or(given, |(flag, _)| flag);
    let name = flag.strip_prefix("--")?;
    let other = if name.contains('_') {
        name.replace('_', "-")
    } else {
        name.replace('-', "_")
    };
    let respelled = format!("--{other}");
    if *suggested != respelled || other == name {
        return None;
    }
    let mut line = arguments.to_vec();
    let word = line.get(at)?.to_str()?.replacen(flag, &respelled, 1);
    *line.get_mut(at)? = OsString::from(word);
    Some(line)
}

/// `arguments` with the `--path` at `at` dropped, so its value is the
/// positional path, when the refused command is a built-in command that
/// takes a path as a positional argument and has no `--path` of its own.
fn path(arguments: &[OsString], refused: &crumbkit::Refused, at: usize) -> Option<Vec<OsString>> {
    let word = arguments.get(at)?.to_str()?;
    let value = match word.split_once('=') {
        Some(("--path", value)) => Some(value.to_owned()),
        None if word == "--path" => None,
        _ => return None,
    };
    if refused.path().is_empty() {
        return None;
    }
    let command = (refused.path().iter()).try_fold(Cli::command(), |command, name| {
        command.find_subcommand(name).cloned()
    })?;
    if !positional_path(&command) {
        return None;
    }
    let mut line = arguments.to_vec();
    match value {
        Some(value) => *line.get_mut(at)? = OsString::from(value),
        None => {
            line.remove(at);
        }
    }
    Some(line)
}

/// Whether `command` takes its path as a positional argument named `path`
/// or `paths`, and no flag named `--path`.
fn positional_path(command: &Command) -> bool {
    let named = |id: &str| id == "path" || id == "paths";
    command
        .get_positionals()
        .any(|arg| named(arg.get_id().as_str()))
        && !command
            .get_arguments()
            .any(|arg| arg.get_long() == Some("path"))
}
