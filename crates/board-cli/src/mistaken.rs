//! Refusals of mistakes agents make often, which name the command they
//! meant (STANDARDS.md 25): Board's wording of clap's refusal of such a
//! command line, and the refusal of a put or rm path that names a folder.

use std::ffi::OsString;

use board_model::{Code, Fields, LogicalPath, Refusal};
use clap::error::{ContextKind, ContextValue, ErrorKind};

use clap::Parser as _;
use crumbkit::help::quote;

use crate::render::{Printer, refused};
use crate::report::Output;
use crate::run::{Cli, Given, Prefix, plain, read};

/// Each first word that is no command, the command meant by it, and what
/// that command does. An alias of the word wins, because Board reads
/// aliases first.
const MEANT: &[(&str, &str, &str)] = &[("cat", "get", "prints the file at a path")];

/// The refusal of `arguments` that names the command meant, or `None` when
/// clap's refusal stands.
pub(crate) fn hinted(arguments: &[OsString], refused_line: &crumbkit::Refused) -> Option<Output> {
    let error = refused_line.error()?;
    let refusal = match (error.kind(), refused_line.path()) {
        (ErrorKind::InvalidSubcommand, []) => {
            let Some(ContextValue::String(word)) = error.get(ContextKind::InvalidSubcommand) else {
                return None;
            };
            unknown(arguments, word)?
        }
        (ErrorKind::UnknownArgument, [put]) if put == "put" => {
            let Some(ContextValue::String(value)) = error.get(ContextKind::InvalidArg) else {
                return None;
            };
            put_input(arguments, value)?
        }
        _ => return None,
    };
    let printer = Printer {
        view: refused_line.output().clone(),
    };
    Some(refused(printer, refusal))
}

/// The refusal of a first word in `MEANT`, whose hint runs the command meant
/// with the same words, or its help when no word follows.
fn unknown(arguments: &[OsString], word: &str) -> Option<Refusal> {
    let (_, command, does) = MEANT.iter().find(|(mistake, ..)| *mistake == word)?;
    let (at, rest) = crate::alias::first_word(arguments)?;
    let mut line = arguments.to_vec();
    *line.get_mut(at).filter(|given| *given == word)? = OsString::from(command);
    if rest.is_empty() {
        line.push("-h".into());
    }
    let message = format!("board has no command {word}; {command} {does}");
    Some(usage(&message, format!("run `{}`", shown(&line)), word))
}

/// The refusal of `put` given `value`, the name of its input, after its
/// path. Put reads its input from stdin, so the hint redirects `value`.
/// `None` when the line without `value` is still refused, such as for a
/// second stray word.
fn put_input(arguments: &[OsString], value: &str) -> Option<Refusal> {
    if value.starts_with('-') {
        return None;
    }
    let mut line = arguments.to_vec();
    line.remove(line.iter().rposition(|given| given == value)?);
    Cli::try_parse_from(&line).ok()?;
    let message =
        format!("put was given {value} after its path, but reads what it writes from stdin");
    // `--new` takes the folder as put's path, so a line with both is refused.
    if line.iter().any(|word| word == "--new") {
        return None;
    }
    let input = quote(value);
    let record = format!("`{} < {input}`", shown(&line));
    let hint = if line.iter().any(|word| word == "--file") {
        format!("pipe {value} to put: run {record}")
    } else {
        // Before any `--`, after which every word is a value.
        let at = line.iter().position(|word| word == "--");
        line.insert(at.unwrap_or(line.len()), "--file".into());
        let file = shown(&line);
        format!(
            "pipe {value} to put: run `{file} < {input}` to write it as a file, \
             or {record} if it holds a JSON record"
        )
    };
    Some(usage(&message, hint, value))
}

/// The file `given` names for `name`, put or rm, read from `prefix`, or a
/// refusal when it ends in `/`, which names a folder.
pub(crate) fn file_path(
    prefix: Option<&Prefix>,
    given: &Given,
    name: &str,
) -> Result<LogicalPath, Box<Refusal>> {
    let Some(folder) = given.0.strip_suffix('/') else {
        return read(prefix, given, name);
    };
    let message = format!("{name}'s path {} ends in /, which names a folder", given.0);
    let hint = if name == "rm" {
        format!(
            "leave out the slash to remove a file, or run `board rm -r {folder}` to remove a folder"
        )
    } else {
        format!(
            "use `--new {}` to add a record in it, or leave out the slash to write a file",
            given.0
        )
    };
    Err(Box::new(plain(Code::CliUsage, &message).with_hint(hint)))
}

/// `line`, whose first word is the binary, as a `board` command line with
/// each word quoted for a shell.
fn shown(line: &[OsString]) -> String {
    let words = line
        .iter()
        .skip(1)
        .map(|word| quote(&word.to_string_lossy()));
    std::iter::once("board".to_owned())
        .chain(words)
        .collect::<Vec<_>>()
        .join(" ")
}

/// A `board::cli::usage` refusal saying `message`, with `hint`, naming
/// `value` in its fields.
fn usage(message: &str, hint: String, value: &str) -> Refusal {
    let mut refusal = Refusal::new(Code::CliUsage, message, &Fields::default()).with_hint(hint);
    refusal.fields.insert("value".to_owned(), value.into());
    refusal
}
