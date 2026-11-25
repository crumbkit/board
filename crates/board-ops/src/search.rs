//! `board search`: a query in Board's search syntax ([`board_search`]),
//! compiled to one SQL statement over the `entries` view and run through
//! the same snapshot as `board query` (docs/commands/search.md).
//!
//! A word matches, in any ASCII case, as a substring of the entry's path,
//! its title, its body and its text: the prose of a Markdown file, or the
//! text of a JSON file with no `$type`. A phrase matches as one string. The match is
//! SQLite's `LIKE`, which folds ASCII case as it compares, so no lower-case
//! copy of a body is made. `type:` takes
//! the short names of [`board_search::fields::TYPES`], each matching every
//! version of its type, and `type:doc` the entries with no `$type`.
//! `from:` matches `by.person`, `by.agent` or `by.program`, and `from:me`
//! the owner the known boards file names. `path:` matches the start of the
//! logical path. A leading minus wraps its part in `NOT`.
//!
//! No text from the query reaches the statement as text. Every value goes
//! in as a blob literal of its UTF-8 bytes in hex, cast to text, so the
//! statement holds only hex digits where the query had words.

use std::fmt::Write as _;
use std::path::Path;

use board_search::fields::{self, short_type};
use board_search::{Field, Part, Query, QueryError as SyntaxError, Term};
use serde_json::Value;

use crate::board::Board;
use crate::query::{QueryError, retried};

/// The most results `search` returns unless asked otherwise.
pub const SEARCH_LIMIT: usize = 50;
/// The most results one page holds.
pub const SEARCH_MAX: usize = 1000;
/// The most characters of a title a result carries.
pub const TITLE_CHARS: usize = 300;

/// One entry that matched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    /// The entry's logical path.
    pub path: String,
    /// The entry's `$type`, when it has one.
    pub kind: Option<String>,
    /// The record's title, or the field that stands for one; for an entry
    /// with no `$type`, its first heading or its file name. Cut to
    /// [`TITLE_CHARS`] characters.
    pub title: Option<String>,
    /// How many of the query's words and phrases its title holds.
    pub title_hits: i64,
    /// How many of the query's words and phrases its body or text holds.
    pub body_hits: i64,
    /// When the working tree last changed the file, in nanoseconds since
    /// the epoch, or [`i64::MIN`] for an entry of a mount.
    pub modified_ns: i64,
}

/// Where a page of results ends: the sort key of its last result. The next
/// page starts after it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cursor {
    title_hits: i64,
    body_hits: i64,
    modified_ns: i64,
    path: String,
}

impl Cursor {
    /// The cursor after `found`.
    pub fn after(found: &Hit) -> Self {
        Self {
            title_hits: found.title_hits,
            body_hits: found.body_hits,
            modified_ns: found.modified_ns,
            path: found.path.clone(),
        }
    }

    /// The cursor as `board search --after` takes it: the two counts, the
    /// modification time and the path in hex, separated by dots, such as
    /// `1.2.1700000000000000000.2f7461736b73`.
    pub fn text(&self) -> String {
        format!(
            "{}.{}.{}.{}",
            self.title_hits,
            self.body_hits,
            self.modified_ns,
            hex(self.path.as_bytes())
        )
    }

    /// The cursor that `text` spells, or `None` when it is not one that
    /// [`Cursor::text`] printed.
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.splitn(4, '.');
        let title_hits = parts.next()?.parse().ok()?;
        let body_hits = parts.next()?.parse().ok()?;
        let modified_ns = parts.next()?.parse().ok()?;
        let path = unhex(parts.next()?)?;
        Some(Self {
            title_hits,
            body_hits,
            modified_ns,
            path,
        })
    }
}

/// What a search returned.
#[derive(Clone, Debug, PartialEq)]
pub struct SearchReport {
    pub results: Vec<Hit>,
    /// Where the next page starts, when more records match.
    pub next: Option<Cursor>,
    pub memory: Option<board_index::Fallback>,
}

impl Board {
    /// Runs `query` over every entry of the board and its mounts, through
    /// the index kept under `cache`, returning at most `limit` results that
    /// come after `after`. The statement is the one [`compile`] gives.
    pub fn search(
        &self,
        statement: &Statement,
        limit: usize,
        after: Option<&Cursor>,
        cache: Option<&Path>,
    ) -> Result<SearchReport, QueryError> {
        let sql = statement.sql(after);
        let report = retried(|| {
            let (view, memory) = self.decided(cache)?;
            self.run(&view, memory, &sql, limit, None)
        })?;
        let results: Vec<Hit> = report.rows.iter().filter_map(|row| found(row)).collect();
        let next = (report.truncated)
            .then(|| results.last().map(Cursor::after))
            .flatten();
        Ok(SearchReport {
            results,
            next,
            memory: report.memory,
        })
    }
}

/// A result from one row of the statement: path, type, title, the two
/// counts and the modification time.
fn found(row: &[Value]) -> Option<Hit> {
    let text = |at: usize| row.get(at).and_then(Value::as_str).map(str::to_owned);
    let count = |at: usize| row.get(at).and_then(Value::as_i64).unwrap_or(0);
    Some(Hit {
        path: text(0)?,
        kind: text(1),
        title: text(2),
        title_hits: count(3),
        body_hits: count(4),
        modified_ns: row.get(5).and_then(Value::as_i64).unwrap_or(i64::MIN),
    })
}

/// A parsed query ready to run: its filters and the words it ranks by.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Statement {
    /// The `WHERE` condition, or `1` for a query with no parts.
    condition: String,
    /// The words and phrases that are not excluded, which the ranking
    /// counts.
    ranked: Vec<String>,
}

/// The statement for `query`, with `from:me` read as `owner`, the owner the
/// known boards file names, or `None` when it names none.
///
/// # Errors
///
/// Returns every `type:` value that names no short type, every `path:`
/// value that does not start with `/` and every `from:me` when `owner` is
/// `None`, in the order of the text, so nothing runs for a query with an
/// error.
pub fn compile(query: &Query, owner: Option<&str>) -> Result<Statement, Vec<SyntaxError>> {
    let mut errors = fields::check(query);
    let (mut filters, mut words) = (Vec::new(), Vec::new());
    let mut ranked = Vec::new();
    for part in query.parts() {
        let texts = matches!(part.term, Term::Word { .. } | Term::Phrase { .. });
        match condition(part, owner) {
            Ok(condition) if texts => words.push(condition),
            Ok(condition) => filters.push(condition),
            Err(mut unknown) => errors.append(&mut unknown),
        }
        if let (false, Term::Word { text } | Term::Phrase { text }) = (part.negated, &part.term) {
            ranked.push(text.clone());
        }
    }
    if !errors.is_empty() {
        errors.sort_by_key(|error| error.span.start);
        return Err(errors);
    }
    // The filters come first, so an entry they rule out is never searched
    // for the words, which read the whole body and text.
    filters.append(&mut words);
    let conditions = filters;
    let condition = if conditions.is_empty() {
        "1".to_owned()
    } else {
        conditions.join(" AND ")
    };
    Ok(Statement { condition, ranked })
}

/// The SQL condition of one part, or the `from:me` values that name nobody
/// because `owner` is `None`. A `type:` value that names no short type, or
/// a `path:` value that does not start with `/`, matches nothing;
/// [`fields::check`] reports it.
fn condition(part: &Part, owner: Option<&str>) -> Result<String, Vec<SyntaxError>> {
    let matched = match &part.term {
        Term::Word { text } | Term::Phrase { text } => {
            let (body, prose, path) = (holds("b", text), holds("x", text), holds("path", text));
            let title = holds("t", text);
            if in_body(text) {
                format!("({body} OR {prose} OR {path} OR (k = '' AND {title}))")
            } else {
                format!("({body} OR {prose} OR {path} OR {title})")
            }
        }
        Term::Filter {
            field: Field::Type,
            values,
        } => {
            let any: Vec<String> = values
                .iter()
                .filter_map(|value| short_type(&value.text))
                .map(|short| match short.prefix {
                    Some(prefix) => {
                        let length = prefix.len();
                        format!("substr(k, 1, {length}) = {}", literal(prefix))
                    }
                    None => "k = ''".to_owned(),
                })
                .collect();
            any_of(&any)
        }
        Term::Filter {
            field: Field::Path,
            values,
        } => {
            let any: Vec<String> = values
                .iter()
                .filter(|value| value.text.starts_with('/'))
                .map(|value| {
                    let length = value.text.chars().count();
                    format!("substr(path, 1, {length}) = {}", literal(&value.text))
                })
                .collect();
            any_of(&any)
        }
        Term::Filter {
            field: Field::From,
            values,
        } => {
            let mut unknown = Vec::new();
            let mut any = Vec::new();
            for value in values {
                let me = value.text.eq_ignore_ascii_case("me");
                let Some(name) = (if me { owner } else { Some(value.text.as_str()) }) else {
                    unknown.push(fields::unknown_person(value));
                    continue;
                };
                let name = literal(&name.to_ascii_lowercase());
                any.push(format!(
                    "person = {name} OR agent = {name} OR program = {name}"
                ));
            }
            if !unknown.is_empty() {
                return Err(unknown);
            }
            any_of(&any)
        }
    };
    Ok(if part.negated {
        format!("NOT {matched}")
    } else {
        matched
    })
}

/// SQL that is 1 when `column` holds `text` in any ASCII case and 0 when it
/// does not: `LIKE`, with each `%`, `_` and `\` in `text` escaped by `\`.
fn holds(column: &str, text: &str) -> String {
    let mut pattern = String::with_capacity(text.len() + 2);
    pattern.push('%');
    for character in text.chars() {
        if matches!(character, '%' | '_' | '\\') {
            pattern.push('\\');
        }
        pattern.push(character);
    }
    pattern.push('%');
    format!(
        "({column} LIKE {} ESCAPE {})",
        literal(&pattern),
        literal("\\")
    )
}

/// Whether a record's title that holds `text` always has a body that holds
/// it too, so the title need not be read to decide a match. The body is the
/// record's JSON, which writes a string as it is apart from `"`, `\` and
/// control characters, so a title holds such text only when the body does.
/// An entry with no `$type` has a heading or a file name for a title, which
/// is always read.
fn in_body(text: &str) -> bool {
    !text
        .chars()
        .any(|character| matches!(character, '"' | '\\') || character.is_control())
}

/// `conditions` joined by `OR` in parentheses, or `0` when there are none.
fn any_of(conditions: &[String]) -> String {
    if conditions.is_empty() {
        "(0)".to_owned()
    } else {
        format!("({})", conditions.join(" OR "))
    }
}

/// The entries with the columns the conditions read: `path`; `t`, the
/// title; `b`, a record's body; `x`, the text of a Markdown file or of a
/// JSON file with no `$type`; `k`, the type; `m`, when the working tree last
/// changed the file, or the smallest integer for an entry of a mount; and
/// `person`, `agent` and `program`, from `by`, in lower case. Each is empty
/// text where the entry has none. A record's title is the first of
/// `title`, `summary`, `decision`, `words` and `question` that it has, so a
/// question or a decision has one. An entry with no `$type` is titled by
/// its first heading, or by its file name when it has none. A word's
/// condition reads the body, the text and the path before the title,
/// because a record's title costs a read of the JSON for each record.
const ENTRIES: &str = "\
WITH e AS (
    SELECT e.source, e.path AS physical, e.logical AS path, e.blob, e.kind, e.role, e.type
    FROM entries AS e WHERE e.logical IS NOT NULL
), r AS (
    SELECT e.path, e.type, b.body, x.body AS prose,
        CASE WHEN e.type IS NULL
            THEN coalesce(x.heading,
                substr(e.path, length(rtrim(e.path, replace(e.path, '/', ''))) + 1))
            ELSE CAST(coalesce(json_extract(b.body, '$.title'), json_extract(b.body, '$.summary'),
                json_extract(b.body, '$.decision'), json_extract(b.body, '$.words'),
                json_extract(b.body, '$.question')) AS TEXT)
        END AS title,
        coalesce(w.modified_ns, -9223372036854775808) AS m
    FROM e
    LEFT JOIN main.bodies AS b ON e.kind = 'record' AND b.blob = e.blob
    LEFT JOIN main.texts AS x ON x.blob = e.blob
        AND (e.role = 'markdown' OR (e.role = 'json' AND e.type IS NULL))
    LEFT JOIN main.worktree AS w ON e.source = 'worktree' AND w.path = e.physical
), c AS (
    SELECT path, type, title, m,
        coalesce(title, '') AS t, coalesce(body, '') AS b, coalesce(prose, '') AS x,
        coalesce(type, '') AS k,
        lower(coalesce(json_extract(body, '$.by.person'), '')) AS person,
        lower(coalesce(json_extract(body, '$.by.agent'), '')) AS agent,
        lower(coalesce(json_extract(body, '$.by.program'), '')) AS program
    FROM r
)";

impl Statement {
    /// The SQL: each matching entry's path, type, title cut to
    /// [`TITLE_CHARS`], how many ranked words its title and its body or text
    /// hold, and when the working tree last changed it: most title words
    /// first, then most body words, then newest, then by path, starting
    /// after `after`.
    pub fn sql(&self, after: Option<&Cursor>) -> String {
        let hits = |held: &dyn Fn(&str) -> String| {
            if self.ranked.is_empty() {
                return "0".to_owned();
            }
            let counted: Vec<String> = (self.ranked.iter()).map(|word| held(word)).collect();
            counted.join(" + ")
        };
        let titled = hits(&|word| holds("t", word));
        let bodied = hits(&|word| format!("({} OR {})", holds("b", word), holds("x", word)));
        let mut sql = format!(
            "{ENTRIES}, s AS (
    SELECT path, type, substr(title, 1, {TITLE_CHARS}) AS title,
        {titled} AS title_hits, {bodied} AS body_hits, m
    FROM c WHERE {}
)
SELECT path, type, title, title_hits, body_hits, m FROM s",
            self.condition
        );
        if let Some(after) = after {
            let (title, body) = (after.title_hits, after.body_hits);
            let (modified, path) = (after.modified_ns, literal(&after.path));
            let _ = write!(
                sql,
                "\nWHERE title_hits < {title} OR (title_hits = {title} AND (body_hits < {body} \
                 OR (body_hits = {body} AND (m < {modified} \
                 OR (m = {modified} AND path > {path})))))"
            );
        }
        sql.push_str("\nORDER BY title_hits DESC, body_hits DESC, m DESC, path");
        sql
    }
}

/// `text` as SQL that reads back as the same text: a blob literal of its
/// UTF-8 bytes, cast to text. It holds only hex digits, so no character of
/// `text` reaches the statement.
fn literal(text: &str) -> String {
    format!("CAST(X'{}' AS TEXT)", hex(text.as_bytes()))
}

/// `bytes` in lower-case hex.
fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(text, "{byte:02x}");
    }
    text
}

/// The text that the lower-case or upper-case hex `text` spells, or `None`
/// when it is not hex of UTF-8.
fn unhex(text: &str) -> Option<String> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    let bytes: Option<Vec<u8>> = (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(text.get(at..at + 2)?, 16).ok())
        .collect();
    String::from_utf8(bytes?).ok()
}

#[cfg(test)]
mod tests;
