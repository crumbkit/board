//! Board's wording of clap's refusal of a command line that makes a
//! mistake agents make often, which names the command they meant
//! (STANDARDS.md 25).

use std::ffi::OsString;

use board_model::{Code, Fields, Refusal};
use clap::error::{ContextKind, ContextValue, ErrorKind};

use crate::render::{Printer, refused};
use crate::report::Output;

/// Each first word that is no command, the command meant by it, and what
/// that command does. An alias of the word wins, because Board reads
/// aliases first.
const MEANT: &[(&str, &str, &str)] = &[("cat", "get", "prints the file at a path")];

/// The refusal of `arguments` that names the command meant, or `None` when
/// clap's refusal stands.
pub(crate) fn hinted(arguments: &[OsString], refused_line: &crumbkit::Refused) -> Option<Output> {
    let error = refused_line.error()?;
    let words: Vec<String> = arguments
        .iter()
        .skip(1)
        .map(|word| word.to_string_lossy().into_owned())
        .collect();
    let refusal = match (error.kind(), refused_line.path()) {
        (ErrorKind::InvalidSubcommand, []) => {
            let Some(ContextValue::String(word)) = error.get(ContextKind::InvalidSubcommand) else {
                return None;
            };
            unknown(&words, word)?
        }
        (ErrorKind::UnknownArgument, [put]) if put == "put" => {
            let Some(ContextValue::String(value)) = error.get(ContextKind::InvalidArg) else {
                return None;
            };
            if value.starts_with('-') {
                return None;
            }
            put_input(&words, value)
        }
        _ => return None,
    };
    let printer = Printer {
        view: refused_line.output().clone(),
    };
    Some(refused(printer, refusal))
}

/// The refusal of a first word in `MEANT`, whose hint runs the command meant
/// with the same words.
fn unknown(words: &[String], word: &str) -> Option<Refusal> {
    let (_, command, does) = MEANT.iter().find(|(mistake, ..)| *mistake == word)?;
    let at = words.iter().position(|given| given == word)?;
    let mut line = words.to_vec();
    (*command).clone_into(&mut line[at]);
    let message = format!("board has no command {word}; {command} {does}");
    let hint = format!("run `board {}`", line.join(" "));
    Some(usage(&message, hint, word))
}

/// The refusal of `put` given `value`, the name of its input, after its
/// path. Put reads its input from stdin, so the hint redirects `value`.
fn put_input(words: &[String], value: &str) -> Refusal {
    let mut line = words.to_vec();
    if let Some(at) = line.iter().rposition(|given| given == value) {
        line.remove(at);
    }
    let line = format!("board {}", line.join(" "));
    let message =
        format!("put was given {value} after its path, but reads what it writes from stdin");
    let chosen = ["--file", "--new", "--batch"]
        .iter()
        .any(|flag| words.iter().any(|word| word == flag));
    let hint = if chosen {
        format!("pipe {value} to put: run `{line} < {value}`")
    } else {
        format!(
            "pipe {value} to put: run `{line} --file < {value}` to write it as a file, \
             or `{line} < {value}` if it holds a JSON record"
        )
    };
    usage(&message, hint, value)
}

/// A `board::cli::usage` refusal saying `message`, with `hint`, naming
/// `value` in its fields.
fn usage(message: &str, hint: String, value: &str) -> Refusal {
    let mut refusal = Refusal::new(Code::CliUsage, message, &Fields::default()).with_hint(hint);
    refusal.fields.insert("value".to_owned(), value.into());
    refusal
}
