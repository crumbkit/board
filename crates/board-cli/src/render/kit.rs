//! Printing a result through crumbkit, in crumbkit's own formats.

use std::fmt;
use std::fmt::Write as _;

use board_model::Refusal;
use crumbkit::help::{Guide, RefusalContext};
use crumbkit::{
    Batch, Call, Code, Document, Failure, Field, FieldValue, Listing, Prose, Recovery, Row, Rows,
    Sink, Spelling, codes,
};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;

use crate::report::Output;

/// How Board prints: its budget, and the same text on a terminal as in a
/// pipe.
pub(crate) const GUIDE: Guide = guide(super::BUDGET);

const fn guide(budget: usize) -> Guide {
    Guide::new()
        .output_budget(
            budget,
            "a reader asks again for the paths and rows a run leaves out",
        )
        .without_terminal_detection("Board's tests read its text from a pipe")
        .refusal_alternatives(path_fixed)
        .leaves(super::LEAVES)
}

/// For a path refused only for its form, the refused line with the path
/// given its leading `/` and without `.json`. crumbkit offers it when that
/// line parses.
fn path_fixed(context: &RefusalContext<'_>) -> Vec<Call> {
    let (Some("path" | "paths"), Some(value)) = (context.argument(), context.value()) else {
        return Vec::new();
    };
    let stem = value.strip_suffix(".json").unwrap_or(value);
    let mut words: Vec<String> = (context.words().iter())
        .map(|word| word.to_string_lossy().into_owned())
        .collect();
    let Some(at) = words.iter().position(|word| word == value) else {
        return Vec::new();
    };
    words[at] = format!("/{}", stem.trim_start_matches('/'));
    vec![Call::typed(words)]
}

/// A Board refusal, as crumbkit prints a failure.
#[derive(Debug)]
pub(crate) struct Refused(pub(crate) Refusal);

impl Refused {
    pub(crate) const fn new(refusal: Refusal) -> Self {
        Self(refusal)
    }
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0.message)
    }
}

impl std::error::Error for Refused {}

/// The code of a refusal whose code crumbkit cannot hold.
const UNHELD: Code = Code::new("board", "output", "unheld");

impl Failure for Refused {
    fn code(&self) -> &'static Code {
        let code = self.0.code.as_str();
        // crumbkit exits 2 only for its own usage code.
        if code == board_model::Code::CliUsage.as_str() {
            return &codes::CLI_USAGE;
        }
        Spelling::COLONS
            .split(code)
            .and_then(|(component, area, reason)| Code::runtime(component, area, reason).ok())
            .unwrap_or(&UNHELD)
    }

    /// A refusal keeps its hint as text.
    fn recovery(&self) -> Recovery {
        self.0.hint.clone().map_or_else(
            || Recovery::None {
                reason: "the plugin gave no hint".to_owned(),
            },
            Recovery::Hint,
        )
    }

    fn fields(&self) -> Vec<Field<'_>> {
        (self.0.fields.iter())
            .filter_map(|(name, value)| Some(Field::new(name, FieldValue::from_json(value)?)))
            .collect()
    }
}

/// A value Board builds as JSON, which prints as `text`: a command's
/// answer as prose, or a listing's summary.
#[derive(Serialize, JsonSchema)]
#[serde(transparent)]
pub(crate) struct Said {
    value: Value,
    #[serde(skip)]
    text: String,
}

impl Said {
    pub(crate) const fn new(value: Value, text: String) -> Self {
        Self { value, text }
    }
}

impl fmt::Display for Said {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

/// A row Board builds as JSON, printed whole. As text it prints its own
/// line, or its JSON on one line when it has none.
#[derive(Serialize, JsonSchema)]
#[serde(transparent)]
pub(crate) struct Whole {
    value: Value,
    #[serde(skip)]
    line: Option<String>,
}

impl Whole {
    pub(crate) const fn new(value: Value) -> Self {
        Self { value, line: None }
    }

    pub(crate) const fn lined(value: Value, line: String) -> Self {
        Self {
            value,
            line: Some(line),
        }
    }
}

impl Row for Whole {
    const COLUMNS: &'static [&'static str] = &[];
    const WHOLE: bool = true;
    const LINE: Option<&'static str> = Some("<ENTRY>");

    fn text_line(&self) -> Option<String> {
        self.line.clone()
    }
}

/// A [`Whole`] row whose `code`, when it has one, reports a problem, so a
/// listing of them exits 1.
#[derive(Serialize, JsonSchema)]
#[serde(transparent)]
pub(crate) struct Flagged(pub(crate) Whole);

impl Row for Flagged {
    const COLUMNS: &'static [&'static str] = &["code"];
    const WHOLE: bool = true;
    const LINE: Option<&'static str> = Some("<ENTRY>");
    const PROBLEM: Option<&'static str> = Some("code");

    fn text_line(&self) -> Option<String> {
        self.0.text_line()
    }

    /// As crumbkit's default reads `code`, without serializing the row.
    fn is_problem(&self) -> bool {
        match self.0.value.get("code") {
            None | Some(Value::Null | Value::Bool(false)) => false,
            Some(Value::String(text)) => !text.is_empty(),
            Some(_) => true,
        }
    }
}

/// One line of a note on stderr beside a result.
pub(crate) enum Note {
    /// Says only what the JSON document holds, so it prints only as text.
    Text(String),
    /// Says what the document does not hold, such as that the index was in
    /// memory, so it prints in either format.
    Always(String),
}

/// A result that carries crumbkit's note.
pub(crate) trait Noted: Rows {
    fn noted(self, text: String) -> Self;
}

impl<R: Row> Noted for Listing<R, Said> {
    fn noted(self, text: String) -> Self {
        self.note(text)
    }
}

impl Noted for Prose<Said, Refused> {
    fn noted(self, text: String) -> Self {
        self.note(text)
    }
}

impl<R: Row> Noted for Batch<R, Refused> {
    fn noted(self, text: String) -> Self {
        self.note(text)
    }
}

/// One run's printer: crumbkit's view of the command line it parsed. It
/// prints one result, so printing takes it by value.
pub(crate) struct Printer {
    pub(crate) view: crumbkit::Output,
}

impl Printer {
    /// A printer for `words`, the whole command line, that prints JSON when
    /// `json` is set and prints by `guide`.
    pub(crate) fn new(
        json: bool,
        guide: &Guide,
        words: impl IntoIterator<Item = impl Into<std::ffi::OsString>>,
    ) -> Self {
        use clap::CommandFactory as _;
        let view = crumbkit::Output::new_to(
            &mut Sink::new(std::io::sink(), std::io::sink()),
            crumbkit::ViewFlags::new().json(json),
            crumbkit::DryRun::default(),
            crate::run::Cli::command(),
            guide,
            words,
        )
        .expect("crumbkit refuses only --full without --json or --fields with --long or --full, and these flags set neither");
        Self { view }
    }

    /// A printer for `words`, the command line after `board`, that prints
    /// JSON when `json` is set and has `budget` bytes.
    #[cfg(test)]
    pub(crate) fn of(json: bool, budget: usize, words: &[&str]) -> Self {
        let args = std::iter::once("board").chain(words.iter().copied());
        Self::new(json, &guide(budget), args)
    }

    pub(crate) const fn json(&self) -> bool {
        self.view.is_json()
    }

    /// `result` printed as `document`. A refusal's lines of a batch and a
    /// plugin's stderr, which crumbkit's text does not print, follow it.
    pub(crate) fn print<T: Rows>(
        self,
        document: Document<T>,
        result: Result<T, Refusal>,
    ) -> Output {
        self.finish(document, result.map_err(Refused::new))
    }

    /// `value` printed as `document`, with `notes`, in order, as crumbkit's
    /// note on stderr in text.
    ///
    /// Glue: crumbkit's JSON document has no place for a note, so with
    /// `--json` Board prints each [`Note::Always`] on stderr itself.
    pub(crate) fn noted<T: Noted>(
        self,
        document: Document<T>,
        value: T,
        notes: Vec<Note>,
    ) -> Output {
        if self.json() {
            let mut output = self.print(document, Ok(value));
            for note in notes {
                if let Note::Always(line) = note {
                    output.stderr.extend(format!("{line}\n").into_bytes());
                }
            }
            return output;
        }
        let lines: Vec<String> = (notes.into_iter())
            .map(|(Note::Text(line) | Note::Always(line))| line)
            .collect();
        let value = if lines.is_empty() {
            value
        } else {
            value.noted(lines.join("\n"))
        };
        self.print(document, Ok(value))
    }

    fn finish<T: Rows>(self, document: Document<T>, result: Result<T, Refused>) -> Output {
        let error = match &result {
            Ok(value) => first_code(value),
            Err(refused) => Some(refused.0.code.as_str().to_owned()),
        };
        let after = match &result {
            Err(refused) if !self.json() => text_only(&refused.0),
            _ => Vec::new(),
        };
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let code = crumbkit::finish_to(
            &mut Sink::new(&mut stdout, &mut stderr),
            self.view,
            document,
            result,
        );
        stderr.extend(after);
        Output {
            stdout,
            stderr,
            code,
            error,
        }
    }
}

/// What a refusal's text adds after crumbkit's: each refused line of a
/// batch, then the end of a plugin's stderr.
fn text_only(refusal: &Refusal) -> Vec<u8> {
    let mut text = String::new();
    for line in &refusal.lines {
        let _ = writeln!(
            text,
            "  line {}: {}: {}",
            line.line, line.code, line.message
        );
    }
    let mut bytes = text.into_bytes();
    if !refusal.stderr.is_empty() {
        bytes.extend_from_slice(&refusal.stderr);
        if !refusal.stderr.ends_with(b"\n") {
            bytes.push(b'\n');
        }
    }
    bytes
}

/// The code of the first failure `value` holds.
fn first_code<T: Rows>(value: &T) -> Option<String> {
    let failures = value.failures()?;
    let first = failures.first()?;
    Some(Spelling::COLONS.spell(first.code()))
}
