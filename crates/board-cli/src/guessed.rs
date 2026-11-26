//! Command lines that agents often guess, read as the one command they
//! mean (docs/plugins/calling/commands.md, Guessed words): a first word
//! such as `cat`, a long flag spelled with hyphens for underscores or the
//! reverse, and `--path` given to a read command whose one positional
//! argument is its path. Board reads them only after clap refused the line, so a line
//! that parses never reaches them, and only where one command fits.

use std::ffi::OsString;

use clap::CommandFactory as _;
use clap::error::{ContextKind, ContextValue, ErrorKind};

use crate::run::Cli;

/// Each first word that is no command, and the built-in command it means.
/// An enabled plugin's name or alias wins, because Board reads those first.
const WORDS: &[(&str, &str)] = &[
    ("cat", "get"),
    ("show", "get"),
    ("list", "ls"),
    ("schema", "schemas"),
];

/// The built-in commands, by their words, that read `--path X` as their
/// path. Each only reads, and takes its path as its one positional
/// argument, so dropping the `--path` word cannot move a value into another
/// argument's place or turn a guess into a write.
const PATH_READERS: &[&[&str]] = &[
    &["get"],
    &["ls"],
    &["check"],
    &["log"],
    &["refs"],
    &["uri"],
    &["mount", "update"],
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
            let (at, flag) = flag_at(arguments, given)?;
            spelled(arguments, error, flag, at).or_else(|| path(arguments, refused, flag, at))
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

/// Where the flag `given` names sits in `arguments`, as itself or as
/// `<flag>=<value>`, before any `--`, after which every word is a value,
/// and the flag without its value.
fn flag_at<'g>(arguments: &[OsString], given: &'g str) -> Option<(usize, &'g str)> {
    let flag = given.split_once('=').map_or(given, |(flag, _)| flag);
    let mut words = arguments.iter().take_while(|word| *word != "--");
    let at = words.position(|word| {
        word.to_str().is_some_and(|word| {
            word == flag
                || word
                    .strip_prefix(flag)
                    .is_some_and(|rest| rest.starts_with('='))
        })
    })?;
    Some((at, flag))
}

/// `arguments` with the long flag `flag` at `at` spelled as the flag clap
/// suggested, when that differs from it only by hyphens for underscores or
/// the reverse.
fn spelled(
    arguments: &[OsString],
    error: &clap::Error,
    flag: &str,
    at: usize,
) -> Option<Vec<OsString>> {
    let Some(ContextValue::String(suggested)) = error.get(ContextKind::SuggestedArg) else {
        return None;
    };
    let name = flag.strip_prefix("--")?;
    let other = if name.contains('_') {
        name.replace('_', "-")
    } else {
        name.replace('-', "_")
    };
    // clap never suggests the flag it refused, so a name with neither
    // separator, whose other spelling is itself, never matches.
    let respelled = format!("--{other}");
    if *suggested != respelled {
        return None;
    }
    let mut line = arguments.to_vec();
    let word = line.get(at)?.to_str()?.replacen(flag, &respelled, 1);
    *line.get_mut(at)? = OsString::from(word);
    Some(line)
}

/// `arguments` with the `--path` at `at` dropped, or `--path=X` replaced
/// by `X`, so its value is the positional path, when the refused command
/// is one of [`PATH_READERS`] and still has no `--path` flag of its own.
fn path(
    arguments: &[OsString],
    refused: &crumbkit::Refused,
    flag: &str,
    at: usize,
) -> Option<Vec<OsString>> {
    let words = refused.path().iter().map(String::as_str);
    let reads = (PATH_READERS.iter()).any(|reader| reader.iter().copied().eq(words.clone()));
    if flag != "--path" || !reads {
        return None;
    }
    let command = (refused.path().iter()).try_fold(Cli::command(), |command, name| {
        command.find_subcommand(name).cloned()
    })?;
    if command.get_positionals().count() != 1
        || command
            .get_arguments()
            .any(|arg| arg.get_long() == Some("path"))
    {
        return None;
    }
    let mut line = arguments.to_vec();
    let word = line.get(at)?.to_str()?;
    match word.strip_prefix("--path=") {
        Some(value) => *line.get_mut(at)? = OsString::from(value.to_owned()),
        None => {
            line.remove(at);
        }
    }
    Some(line)
}
