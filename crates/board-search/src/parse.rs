//! [`parse`]: the text a person types, as a [`Query`] or every error in
//! it.
//!
//! The parser reads the text as Unicode scalar values, so each span counts
//! characters as the case file does. Whitespace separates parts. A part is
//! an optional leading minus, then a phrase in quotes, a filter, or a word
//! that runs to the next whitespace. A filter is a field name of ASCII
//! letters, digits and dashes that starts with a letter, a colon, and one
//! or more values separated by commas, each plain or in quotes. A part
//! ends after a closing quote, so `"a"b` is two parts.
//!
//! Shortcut: a quote cannot appear inside a phrase or a quoted value,
//! because the syntax has no escape. A word or a plain value may hold a
//! quote after its first character.

use crate::error::{ErrorCode, QueryError, message};
use crate::query::{Field, Part, Query, Span, Term, Value};

/// The most characters a search holds, counted in Unicode scalar values.
pub const MAX_CHARS: usize = 1000;

/// Parses `text` into a query.
///
/// # Errors
///
/// Returns every error in the text, in the order of the text, and at most
/// one for each part.
pub fn parse(text: &str) -> Result<Query, Vec<QueryError>> {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() > MAX_CHARS {
        let span = Span::new(MAX_CHARS, chars.len());
        return Err(vec![message(ErrorCode::TooLong, span, None, "")]);
    }
    let mut scanner = Scanner { chars, at: 0 };
    let mut parts = Vec::new();
    let mut errors = Vec::new();
    while scanner.skip_space() {
        match scanner.part() {
            Ok(part) => parts.push(part),
            Err(error) => errors.push(error),
        }
    }
    if errors.is_empty() {
        Ok(Query::new(parts))
    } else {
        Err(errors)
    }
}

/// The text as characters, and the place the next part starts.
struct Scanner {
    chars: Vec<char>,
    at: usize,
}

/// What went wrong in a filter's values: the code and its span.
type ValueError = (ErrorCode, Span);

impl Scanner {
    fn get(&self, index: usize) -> Option<char> {
        self.chars.get(index).copied()
    }

    fn len(&self) -> usize {
        self.chars.len()
    }

    /// The characters from `start` to `end` as text.
    fn text(&self, start: usize, end: usize) -> String {
        self.chars
            .get(start..end)
            .map(|chars| chars.iter().collect())
            .unwrap_or_default()
    }

    /// Moves past whitespace, and says whether a part follows.
    fn skip_space(&mut self) -> bool {
        while self.get(self.at).is_some_and(char::is_whitespace) {
            self.at += 1;
        }
        self.at < self.len()
    }

    /// The first index at or after `from` whose character `stop` accepts,
    /// or the end of the text.
    fn run_end(&self, from: usize, stop: impl Fn(char) -> bool) -> usize {
        (from..self.len())
            .find(|&index| self.get(index).is_some_and(&stop))
            .unwrap_or(self.len())
    }

    /// The index of the next quote at or after `from`.
    fn closing_quote(&self, from: usize) -> Option<usize> {
        (from..self.len()).find(|&index| self.get(index) == Some('"'))
    }

    /// The index of the colon after a field name that starts at `at`, when
    /// one does.
    fn field_colon(&self, at: usize) -> Option<usize> {
        if !self.get(at).is_some_and(|c| c.is_ascii_alphabetic()) {
            return None;
        }
        let end = self.run_end(at, |c| !(c.is_ascii_alphanumeric() || c == '-'));
        (self.get(end) == Some(':')).then_some(end)
    }

    /// Reads the part at the scanner's place and moves past it.
    fn part(&mut self) -> Result<Part, QueryError> {
        let start = self.at;
        let mut at = start;
        let mut negated = false;
        if self.get(at) == Some('-') {
            if self.get(at + 1).is_none_or(char::is_whitespace) {
                self.at = at + 1;
                let span = Span::new(at, at + 1);
                return Err(message(ErrorCode::NothingToExclude, span, None, ""));
            }
            negated = true;
            at += 1;
        }
        let (term, end) = if self.get(at) == Some('"') {
            self.phrase(at)?
        } else if let Some(colon) = self.field_colon(at) {
            self.filter(at, colon)?
        } else {
            let end = self.run_end(at, char::is_whitespace);
            self.at = end;
            (
                Term::Word {
                    text: self.text(at, end),
                },
                end,
            )
        };
        Ok(Part {
            term,
            negated,
            span: Span::new(start, end),
        })
    }

    /// Reads a phrase whose opening quote is at `at`.
    fn phrase(&mut self, at: usize) -> Result<(Term, usize), QueryError> {
        let Some(close) = self.closing_quote(at + 1) else {
            self.at = self.len();
            let span = Span::new(at, self.len());
            return Err(message(ErrorCode::UnclosedQuote, span, None, ""));
        };
        self.at = close + 1;
        let text = self.text(at + 1, close);
        if text.trim().is_empty() {
            let span = Span::new(at, close + 1);
            return Err(message(ErrorCode::EmptyPhrase, span, None, ""));
        }
        Ok((Term::Phrase { text }, close + 1))
    }

    /// Reads a filter whose name starts at `at` and ends at `colon`. The
    /// values of an unknown filter are read and dropped, so a quoted value
    /// gives no second error.
    fn filter(&mut self, at: usize, colon: usize) -> Result<(Term, usize), QueryError> {
        let name = self.text(at, colon);
        let (values, end, error) = self.values(at, colon);
        self.at = end;
        let Some(field) = Field::named(&name) else {
            let span = Span::new(at, colon);
            return Err(message(ErrorCode::UnknownField, span, None, &name));
        };
        if let Some((code, span)) = error {
            return Err(message(code, span, Some(field), &name));
        }
        Ok((Term::Filter { field, values }, end))
    }

    /// Reads the comma-separated values after the colon at `colon` of a
    /// filter whose name starts at `at`: the values, the index after the
    /// last, and the first error. An empty first value names the field and
    /// its colon, and a later empty value names the comma before it.
    fn values(&self, at: usize, colon: usize) -> (Vec<Value>, usize, Option<ValueError>) {
        let mut values = Vec::new();
        let mut error = None;
        let mut next = colon + 1;
        let mut empty = Span::new(at, colon + 1);
        loop {
            match self.get(next) {
                None => {
                    error = error.or(Some((ErrorCode::EmptyValue, empty)));
                    break;
                }
                Some(c) if c.is_whitespace() => {
                    error = error.or(Some((ErrorCode::EmptyValue, empty)));
                    break;
                }
                Some('"') => {
                    let Some(close) = self.closing_quote(next + 1) else {
                        let span = Span::new(next, self.len());
                        error = error.or(Some((ErrorCode::UnclosedQuote, span)));
                        next = self.len();
                        break;
                    };
                    let span = Span::new(next, close + 1);
                    let text = self.text(next + 1, close);
                    if text.is_empty() {
                        error = error.or(Some((ErrorCode::EmptyValue, span)));
                    } else {
                        values.push(Value { text, span });
                    }
                    next = close + 1;
                }
                Some(_) => {
                    let end = self.run_end(next, |c| c.is_whitespace() || c == ',');
                    if end == next {
                        error = error.or(Some((ErrorCode::EmptyValue, empty)));
                    } else {
                        let span = Span::new(next, end);
                        values.push(Value {
                            text: self.text(next, end),
                            span,
                        });
                    }
                    next = end;
                }
            }
            if self.get(next) != Some(',') {
                break;
            }
            empty = Span::new(next, next + 1);
            next += 1;
        }
        (values, next, error)
    }
}

#[cfg(test)]
mod tests;
