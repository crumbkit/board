//! Board's search syntax: [`parse()`] turns the text a person types into a
//! [`Query`] of [`Part`]s, and a query's `Display` prints its canonical
//! text.
//!
//! The first slice of the syntax has plain words, quoted phrases, the
//! `type:` and `from:` filters, a comma inside a filter's value meaning any
//! of, and a leading minus that excludes a part. A space between parts
//! means all of them must match. Any other `field:` is an unknown filter
//! until the increment that adds the field lands.
//!
//! A text with an error gives every error, each with a stable code, the
//! span it covers and one sentence that says how to fix it, and no query,
//! so nothing runs for it. Spans count Unicode scalar values from 0, with
//! an exclusive end, so a parser in another language counts them alike.
//!
//! The syntax is Flare's. `cases/search-cases.json` is a copy of Flare's
//! shared case file, pinned in `cases/pin.json` to the Flare commit it came
//! from, and this parser passes every case in it. [`fields`] holds Board's
//! own filter list and the short type names `type:` takes, which Flare
//! does not share.

mod error;
pub mod fields;
mod parse;
mod query;

pub use error::{ErrorCode, QueryError};
pub use parse::{MAX_CHARS, parse};
pub use query::{Field, Part, Query, Span, Term, Value};
