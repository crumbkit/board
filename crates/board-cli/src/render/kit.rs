//! Printing a result through crumbkit, in crumbkit's own formats.

use std::fmt;
use std::fmt::Write as _;

use board_model::Refusal;
use crumbkit::help::{Guide, Rename};
use crumbkit::{
    Call, Code, Document, Failure, Field, FieldValue, Recovery, Row, Rows, Sink, Spelling, codes,
};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;

use crate::report::Output;

/// Board's code for a command line it refuses.
pub(crate) const USAGE: Code = Code::new("board", "usage", "invalid");

/// How Board prints: its own usage code, its budget, and the same text on a
/// terminal as in a pipe.
pub(crate) const GUIDE: Guide = guide(super::BUDGET);

/// Board's name for crumbkit's usage code.
const RENAMES: &[Rename] = &[Rename::new(&codes::CLI_USAGE, &USAGE)];

const fn guide(budget: usize) -> Guide {
    Guide::new()
        .codes(RENAMES)
        .output_budget(
            budget,
            "a reader asks again for the paths and rows a run leaves out",
        )
        .without_terminal_detection("Board's tests read its text from a pipe")
        .leaves(super::LEAVES)
}

/// A Board refusal, as crumbkit prints a failure, and the words after
/// `board` of a command line to run instead, when there is one.
#[derive(Debug)]
pub(crate) struct Refused(pub(crate) Refusal, Option<Vec<String>>);

impl Refused {
    pub(crate) const fn new(refusal: Refusal) -> Self {
        Self(refusal, None)
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
        if code == board_model::Code::UsageInvalid.as_str() {
            return &USAGE;
        }
        Spelling::COLONS
            .split(code)
            .and_then(|(component, area, reason)| Code::runtime(component, area, reason).ok())
            .unwrap_or(&UNHELD)
    }

    /// A line typed with a path corrected is a call to that line; a
    /// refusal otherwise keeps its hint as text.
    fn recovery(&self) -> Recovery {
        if let Some(words) = &self.1 {
            return Recovery::Run(Call::typed(words.clone()));
        }
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
}

/// One run's printer: crumbkit's view of the command line it parsed.
pub(crate) struct Printer {
    pub(crate) view: crumbkit::Output,
}

impl Printer {
    /// A printer for `words`, the command line after `board`, that prints
    /// JSON when `json` is set and has `budget` bytes.
    #[cfg(test)]
    pub(crate) fn of(json: bool, budget: usize, words: &[&str]) -> Self {
        use clap::CommandFactory as _;
        let args = std::iter::once("board").chain(words.iter().copied());
        Self {
            view: crumbkit::Output::new(
                crumbkit::ViewFlags::new().json(json),
                crate::run::Cli::command(),
                &guide(budget),
                args,
            ),
        }
    }

    pub(crate) const fn json(&self) -> bool {
        self.view.is_json()
    }

    /// `result` printed as `document`. A refusal's lines of a batch and a
    /// plugin's stderr, which crumbkit's text does not print, follow it.
    pub(crate) fn print<T: Rows>(
        &mut self,
        document: Document<T>,
        result: Result<T, Refusal>,
    ) -> Output {
        self.finish(document, result.map_err(Refused::new))
    }

    /// `refusal` printed as `document`, whose recovery is to run `words`,
    /// the command line after `board`.
    pub(crate) fn refuse_running<T: Rows>(
        &mut self,
        document: Document<T>,
        refusal: Refusal,
        words: Vec<String>,
    ) -> Output {
        self.finish(document, Err(Refused(refusal, Some(words))))
    }

    fn finish<T: Rows>(&mut self, document: Document<T>, result: Result<T, Refused>) -> Output {
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
            &mut self.view,
            Spelling::COLONS,
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
