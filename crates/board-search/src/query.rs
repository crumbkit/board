//! The parsed form of a search: [`Query`], its [`Part`]s and their
//! [`Span`]s, and the canonical printer.

use serde::{Deserialize, Serialize};
use std::fmt;

/// The characters a part covers in the text it was parsed from, counted
/// in Unicode scalar values from 0. `end` is exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Span {
    /// The first character.
    pub start: usize,
    /// The character after the last one.
    pub end: usize,
}

impl Span {
    /// The span from `start` to `end`.
    #[must_use]
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
}

/// A filter of the first slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Field {
    /// `type:`, the kind of record, by a short name such as `question`.
    Type,
    /// `from:`, the person, agent or program that wrote the record.
    From,
}

impl Field {
    /// Every field, in the order the field list shows them.
    pub const ALL: [Self; 2] = [Self::Type, Self::From];

    /// The field's name, as the canonical text spells it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Type => "type",
            Self::From => "from",
        }
    }

    /// The field that `name` spells, in any case.
    #[must_use]
    pub fn named(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|field| field.name().eq_ignore_ascii_case(name))
    }
}

/// One value of a filter, with its span. A quoted value's span includes
/// its quotes.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Value {
    /// The value, without quotes.
    pub text: String,
    /// Where it was typed.
    pub span: Span,
}

/// What one part matches.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Term {
    /// A word.
    Word {
        /// The word.
        text: String,
    },
    /// A quoted phrase, which matches as one string.
    Phrase {
        /// The phrase, without quotes.
        text: String,
    },
    /// A filter, which matches when any of its values does.
    Filter {
        /// The field.
        field: Field,
        /// The values, at least one.
        values: Vec<Value>,
    },
}

/// One part of a query: what it matches, whether a leading minus excludes
/// it, and the span it covers, minus included.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Part {
    /// What the part matches.
    #[serde(flatten)]
    pub term: Term,
    /// Whether a leading minus excludes what the part matches.
    pub negated: bool,
    /// Where it was typed.
    pub span: Span,
}

/// A parsed search: its parts, all of which must match.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Query {
    parts: Vec<Part>,
}

impl Query {
    /// A query of `parts`.
    #[must_use]
    pub fn new(parts: Vec<Part>) -> Self {
        Self { parts }
    }

    /// The parts, in the order they were typed.
    #[must_use]
    pub fn parts(&self) -> &[Part] {
        &self.parts
    }
}

/// Whether a value or a phrase must be quoted to parse back as one: it
/// holds a space or a comma, or starts with a quote.
fn needs_quotes(text: &str) -> bool {
    text.starts_with('"') || text.chars().any(|c| c.is_whitespace() || c == ',')
}

/// The canonical text: the parts joined by one space, field names in lower
/// case, and values quoted only when they hold a space or a comma.
impl fmt::Display for Query {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, part) in self.parts.iter().enumerate() {
            if index > 0 {
                formatter.write_str(" ")?;
            }
            if part.negated {
                formatter.write_str("-")?;
            }
            match &part.term {
                Term::Word { text } => formatter.write_str(text)?,
                Term::Phrase { text } => write!(formatter, "\"{text}\"")?,
                Term::Filter { field, values } => {
                    write!(formatter, "{}:", field.name())?;
                    for (index, value) in values.iter().enumerate() {
                        if index > 0 {
                            formatter.write_str(",")?;
                        }
                        if needs_quotes(&value.text) {
                            write!(formatter, "\"{}\"", value.text)?;
                        } else {
                            formatter.write_str(&value.text)?;
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
