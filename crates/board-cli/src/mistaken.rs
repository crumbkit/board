//! Refusals of mistakes agents make often, which name the command they
//! meant (STANDARDS.md 25): Board's wording of clap's refusal of such a
//! command line, such as `ls` given several paths, and the refusal of a put
//! or rm path that names a folder. A guess that only one command fits runs
//! that command instead (`crate::guessed`).

use std::ffi::OsString;

use board_model::{Code, Fields, LogicalPath, Refusal, Role};
use clap::error::{ContextKind, ContextValue, ErrorKind};

use clap::Parser as _;
use crumbkit::help::quote;

use crate::render::{Printer, refused};
use crate::report::Output;
use crate::run::{Cli, Given, Prefix, plain, read};

/// The refusal of `arguments` that names the command meant, or `None` when
/// clap's refusal stands.
pub(crate) fn hinted(arguments: &[OsString], refused_line: &crumbkit::Refused) -> Option<Output> {
    let error = refused_line.error()?;
    let refusal = match (error.kind(), refused_line.path()) {
        (ErrorKind::UnknownArgument, [put]) if put == "put" => {
            let Some(ContextValue::String(value)) = error.get(ContextKind::InvalidArg) else {
                return None;
            };
            put_input(arguments, value)?
        }
        (ErrorKind::UnknownArgument, [ls]) if ls == "ls" => {
            let Some(ContextValue::String(value)) = error.get(ContextKind::InvalidArg) else {
                return None;
            };
            second_path(value)?
        }
        _ => return None,
    };
    let printer = Printer {
        view: refused_line.run().clone(),
    };
    Some(refused(printer, refusal))
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
    // A put of a Markdown path writes its input as a file, or as a record
    // when it is a JSON object, so the line needs no `--file`.
    let markdown = line
        .iter()
        .any(|word| word.to_str().and_then(Role::of) == Some(Role::Markdown));
    let hint = if markdown || line.iter().any(|word| word == "--file") {
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

/// The refusal of `ls` given `value` after its path. `ls` answers one
/// directory with its own page of entries, so it takes one path per call.
/// `None` for a flag `ls` does not take, which clap's refusal names.
fn second_path(value: &str) -> Option<Refusal> {
    if value.starts_with('-') {
        return None;
    }
    let message = format!("ls lists one path per call, and {value} is a second path");
    let hint = format!(
        "run ls once for each path, such as `board ls {}`",
        quote(value)
    );
    Some(usage(&message, hint, value))
}

/// The file `given` names for `name`, put or rm, read from `prefix`, or a
/// refusal when it ends in `/`, which names a folder. `rm -r` removes a
/// folder, so with `recursive` a trailing `/` is read as any path's is.
pub(crate) fn file_path(
    prefix: Option<&Prefix>,
    given: &Given,
    name: &str,
    recursive: bool,
) -> Result<LogicalPath, Box<Refusal>> {
    let Some(folder) = given.0.strip_suffix('/').filter(|_| !recursive) else {
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
