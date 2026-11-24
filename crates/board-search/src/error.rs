//! [`QueryError`]: one error in a search's text, with its code, span and
//! the sentence that says how to fix it.

use crate::query::{Field, Span};
use serde::{Deserialize, Serialize};

/// What is wrong with one span of a search's text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ErrorCode {
    /// A `field:` that the syntax does not have yet.
    UnknownField,
    /// A quote with no closing quote.
    UnclosedQuote,
    /// A filter with no value, or an empty value in a comma list.
    EmptyValue,
    /// A phrase with nothing between its quotes.
    EmptyPhrase,
    /// A minus with nothing after it.
    NothingToExclude,
    /// More than [`MAX_CHARS`](crate::MAX_CHARS) characters.
    TooLong,
    /// A `type:` value that names no kind of record.
    UnknownType,
    /// A `from:` value that names nobody, such as `me` on a board whose
    /// configuration names no owner.
    UnknownPerson,
}

impl ErrorCode {
    /// The code's spelling, such as `unknown_field`, which names no tool,
    /// so every tool that reads the shared case file reads it as it is.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::UnknownField => "unknown_field",
            Self::UnclosedQuote => "unclosed_quote",
            Self::EmptyValue => "empty_value",
            Self::EmptyPhrase => "empty_phrase",
            Self::NothingToExclude => "nothing_to_exclude",
            Self::TooLong => "too_long",
            Self::UnknownType => "unknown_type",
            Self::UnknownPerson => "unknown_person",
        }
    }
}

/// One error: its code, the span it covers and one sentence that says how
/// to fix it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, thiserror::Error)]
#[error("{message}")]
pub struct QueryError {
    /// What is wrong.
    pub code: ErrorCode,
    /// Where.
    pub span: Span,
    /// How to fix it, in one sentence.
    pub message: String,
}

impl QueryError {
    /// The error `code` at `span`, fixed as `message` says.
    #[must_use]
    pub fn new(code: ErrorCode, span: Span, message: impl Into<String>) -> Self {
        Self {
            code,
            span,
            message: message.into(),
        }
    }
}

/// The filters that later increments add, which an unknown filter's
/// message says are not there yet.
const LATER: [&str; 8] = [
    "with",
    "title",
    "in",
    "updated",
    "created",
    "responds-to",
    "is",
    "sort",
];

/// The number of single-character edits that turn `one` into `two`.
fn distance(one: &str, two: &str) -> usize {
    let two: Vec<char> = two.chars().collect();
    let mut previous: Vec<usize> = (0..=two.len()).collect();
    for (row, left) in one.chars().enumerate() {
        let mut current = vec![row + 1];
        for (column, right) in two.iter().enumerate() {
            let replace = previous.get(column).copied().unwrap_or(usize::MAX);
            let delete = previous.get(column + 1).copied().unwrap_or(usize::MAX);
            let insert = current.get(column).copied().unwrap_or(usize::MAX);
            let cost = usize::from(left != *right);
            current.push(
                replace
                    .saturating_add(cost)
                    .min(delete.saturating_add(1))
                    .min(insert.saturating_add(1)),
            );
        }
        previous = current;
    }
    previous.last().copied().unwrap_or(0)
}

/// The sentence for an unknown filter called `name`.
fn unknown_field(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    if LATER.contains(&lower.as_str()) {
        return format!(
            "There is no filter called {name} yet; use type: or from:, or put the text in quotes to search for it."
        );
    }
    let closest = Field::ALL
        .into_iter()
        .map(|field| (distance(&lower, field.name()), field))
        .min_by_key(|(edits, _)| *edits);
    match closest {
        Some((edits, field)) if edits <= 2 => format!(
            "There is no filter called {name}; did you mean {}?",
            field.name()
        ),
        _ => format!(
            "There is no filter called {name}; use type: or from:, or put the text in quotes to search for it."
        ),
    }
}

/// The error `code` at `span`, with the sentence that fixes it. `field` is
/// the filter whose value is empty, and `name` the unknown filter's name
/// or the value that names nothing.
pub(crate) fn message(code: ErrorCode, span: Span, field: Option<Field>, name: &str) -> QueryError {
    let sentence = match (code, field) {
        (ErrorCode::UnknownField, _) => unknown_field(name),
        (ErrorCode::UnclosedQuote, _) => {
            "This quote is not closed; add \" after the last word of the phrase.".to_owned()
        }
        (ErrorCode::EmptyValue, Some(Field::From)) => {
            "from needs a person, an agent or a program, such as from:me or from:claude-code."
                .to_owned()
        }
        (ErrorCode::EmptyValue, _) => {
            "type needs a kind of record, such as type:question.".to_owned()
        }
        (ErrorCode::EmptyPhrase, _) => {
            "This phrase is empty; put words between the quotes, or remove them.".to_owned()
        }
        (ErrorCode::NothingToExclude, _) => {
            "This minus excludes nothing; put it right before a word, a phrase or a filter, or remove it.".to_owned()
        }
        (ErrorCode::TooLong, _) => format!(
            "A search holds at most {} characters; remove the text from here on.",
            crate::MAX_CHARS
        ),
        (ErrorCode::UnknownType, _) => format!(
            "There is no type called {name}; use one of {}.",
            crate::fields::type_names()
        ),
        (ErrorCode::UnknownPerson, _) => format!(
            "from:{name} needs the board's owner; add \"owner\": \"<name>\" to $XDG_CONFIG_HOME/board/config.json, or name the person."
        ),
    };
    QueryError::new(code, span, sentence)
}
