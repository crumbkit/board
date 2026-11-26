//! `board search`: the entries that match a query in Board's search
//! syntax (docs/commands/search.md).

use std::fmt::Write as _;

use board_model::{Code, Refusal, RefusalCode};
use board_ops::{Board, Cursor, Hit, SEARCH_LIMIT, SEARCH_MAX, SearchReport, Statement, compile};
use board_search::fields::{FIELDS, TYPES};
use board_search::{Query, QueryError, parse};
use crumbkit::Document;
use serde_json::{Value, json};

use super::plain;
use crate::config::Context;
use crate::render::{self, Answer, Printer};
use crate::report::Output;

/// How many results one page of `search` holds.
pub(crate) type SearchPage = crumbkit::Page<SEARCH_LIMIT, SEARCH_MAX>;

/// `search`'s arguments.
#[derive(Debug, clap::Args)]
pub(crate) struct Searching {
    /// The query, quoted as one argument, such as 'budget type:question -from:me'.
    #[arg(
        required_unless_present = "filters",
        allow_hyphen_values = true,
        value_name = "QUERY"
    )]
    pub(crate) query: Option<String>,
    /// List the filters and the short type names a query can use, and run no search.
    #[arg(long, conflicts_with_all = ["query", "after"])]
    pub(crate) filters: bool,
    /// Start after the result this cursor names, which the last page printed as `next`.
    #[arg(long, value_name = "CURSOR")]
    pub(crate) after: Option<String>,
    #[command(flatten)]
    pub(crate) page: SearchPage,
}

/// `board search -h`'s syntax and examples.
pub(crate) const SEARCH_HELP: &str = "\
A query is parts separated by spaces, all of which must match:
  budget                 a word in the path, title, body or prose, any case
  \"budget review\"        a phrase, matched as one string
  type:question          task, question, answer, decision, change, plan,
                         eval, run, artifact, rule, or doc for no $type
  from:erin              by.person, by.agent or by.program; from:me is the
                         owner named in $XDG_CONFIG_HOME/board/config.json
  path:/notes            a logical path that starts with /notes
  -type:task             a leading minus excludes the part; a comma in a
                         filter means any of its values

Words match by substring. Results come by the most words in the title,
then in the body, then newest first. docs/commands/search.md says more.

Examples:
  board search 'budget type:question'
  board search 'type:question from:erin -budget'
  board search 'type:doc path:/notes offsite'
  board search --filters";

/// `search`'s document: the canonical query, its parts, the entries that
/// matched and the cursor of the next page; with `--filters`, the filters
/// and the short type names.
pub(crate) const SEARCH: Document<Answer> = Document::new(
    "board",
    "search-report",
    1,
    "The entries a search matched, or with --filters the filters a search can use.",
);

/// The refusal of a query that does not parse, or that names a type or a
/// person that is not there, before any board is read: `board::cli::usage`,
/// which exits 2, with the query and each error in its fields.
pub(crate) fn refused(query: &str, errors: &[QueryError]) -> Refusal {
    let mut message = format!("the search `{query}` cannot run:");
    for error in errors {
        let _ = write!(
            message,
            " {} (characters {} to {})",
            error.message, error.span.start, error.span.end
        );
    }
    let hint = "fix the query at the characters each error names; `board search --filters` lists the filters and the type names";
    let mut refusal = plain(Code::CliUsage, &message).with_hint(hint);
    let listed: Vec<Value> = (errors.iter())
        .map(|error| {
            json!({
                "code": error.code.text(),
                "span": {"start": error.span.start, "end": error.span.end},
                "message": error.message,
            })
        })
        .collect();
    refusal.fields.insert("query".to_owned(), json!(query));
    refusal
        .fields
        .insert("errors".to_owned(), Value::Array(listed));
    refusal
}

/// The query `asked` names, parsed and compiled with the owner the known
/// boards file names, or the refusal that says what is wrong with it.
fn compiled(asked: &Searching, context: &Context) -> Result<(Query, Statement), Box<Refusal>> {
    let text = asked.query.as_deref().unwrap_or_default();
    let query = parse(text).map_err(|errors| Box::new(refused(text, &errors)))?;
    let known = context.known()?;
    let statement =
        compile(&query, known.owner()).map_err(|errors| Box::new(refused(text, &errors)))?;
    Ok((query, statement))
}

/// The cursor `--after` gives, or the refusal of one `search` did not print.
fn cursor(asked: &Searching) -> Result<Option<Cursor>, Box<Refusal>> {
    let Some(text) = asked.after.as_deref() else {
        return Ok(None);
    };
    Cursor::parse(text).map(Some).ok_or_else(|| {
        let message = format!("--after {text} is not a cursor board search printed");
        let hint =
            "pass the `next` value of the page before, or leave out --after for the first page";
        Box::new(plain(Code::CliUsage, &message).with_hint(hint))
    })
}

/// What `search` prints when the board could not be opened, `refused`:
/// the filters, or the refusal of a query or a cursor that cannot run,
/// which need no board, or else `refused`.
pub(crate) fn unopened(
    asked: &Searching,
    context: &Context,
    printer: Printer,
    refused: Refusal,
) -> Output {
    if asked.filters {
        return filters(printer);
    }
    match compiled(asked, context).and_then(|_| cursor(asked)) {
        Ok(_) => render::refused(printer, refused),
        Err(refusal) => render::refused(printer, *refusal),
    }
}

/// `search --filters`: each filter with its kind, values and what it
/// matches, then each short type name with the `$type` prefix it matches.
fn filters(printer: Printer) -> Output {
    let fields: Vec<Value> = (FIELDS.iter())
        .map(|info| {
            json!({
                "name": info.field.name(),
                "kind": info.kind,
                "values": info.values,
                "about": info.about,
            })
        })
        .collect();
    let types: Vec<Value> = (TYPES.iter())
        .map(|short| json!({"name": short.name, "prefix": short.prefix}))
        .collect();
    let mut text = String::from("Filters:\n");
    for info in FIELDS {
        let _ = writeln!(text, "  {}:  {}", info.field.name(), info.about);
    }
    text.push_str("Types:\n");
    for short in TYPES {
        let matched = short
            .prefix
            .map_or("no $type".to_owned(), |prefix| format!("{prefix}*"));
        let _ = writeln!(text, "  {:<9} {matched}", short.name);
    }
    let value = json!({"fields": fields, "types": types});
    printer.print(SEARCH, Ok(render::said(value, text)))
}

/// `search` as `asked`: the records of `board` that match, one per line as
/// text, through the index kept under the context's cache.
pub(crate) fn search(
    board: &Board,
    context: &Context,
    printer: Printer,
    asked: &Searching,
) -> Output {
    if asked.filters {
        return filters(printer);
    }
    let prepared = compiled(asked, context).and_then(|compiled| Ok((compiled, cursor(asked)?)));
    let ((query, statement), after) = match prepared {
        Ok(prepared) => prepared,
        Err(refusal) => return render::refused(printer, *refusal),
    };
    let limit = asked.page.limit().unwrap_or(SEARCH_MAX);
    match board.search(&statement, limit, after.as_ref(), context.cache.as_deref()) {
        Ok(report) => printed(printer, &query, limit, &report),
        Err(error) => render::refused(printer, unanswered(error.refusal())),
    }
}

/// What to try when a search reads for longer than the index allows.
pub(crate) const TIMEOUT_HINT: &str = "add a filter such as type:task or from:erin, or use a longer or less common word, so fewer records match; `board search --filters` lists the filters";

/// `refusal` with a search's recovery in place of `board query`'s when the
/// search ran longer than the index allows, which a person fixes by
/// narrowing the search rather than its SQL.
pub(crate) fn unanswered(refusal: Refusal) -> Refusal {
    if refusal.code == RefusalCode::Board(Code::QueryTimeout) {
        refusal.with_hint(TIMEOUT_HINT)
    } else {
        refusal
    }
}

/// The short name of `kind`, such as `question` for `plans/question/v2`,
/// `doc` for an entry with no `$type`, or `kind` itself when no short name
/// matches it.
fn short(kind: Option<&str>) -> &str {
    let Some(kind) = kind else {
        return "doc";
    };
    (TYPES.iter())
        .find(|short| short.prefix.is_some_and(|prefix| kind.starts_with(prefix)))
        .map_or(kind, |short| short.name)
}

/// One result as JSON: its path, type and title.
fn hit(hit: &Hit) -> Value {
    json!({"path": hit.path, "type": hit.kind, "title": hit.title})
}

/// The report of `query`: as text, one line per result with its path,
/// short type and title, then the command for the next page when there is
/// one, which keeps `limit` when it is not the default.
fn printed(printer: Printer, query: &Query, limit: usize, report: &SearchReport) -> Output {
    let canonical = query.to_string();
    let next = report.next.as_ref().map(Cursor::text);
    let value = json!({
        "query": canonical,
        "parsed": query.parts(),
        "results": report.results.iter().map(hit).collect::<Vec<_>>(),
        "next": next,
    });
    let mut text = String::new();
    for hit in &report.results {
        let kind = short(hit.kind.as_deref());
        let title = hit.title.as_deref().unwrap_or_default();
        let title: String = title
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        let _ = writeln!(text, "{}  {kind}  {title}", hit.path);
    }
    if report.results.is_empty() {
        text.push_str("no record matches\n");
    }
    if let Some(next) = &next {
        let quoted = canonical.replace('\'', r"'\''");
        let _ = write!(text, "next page: board search '{quoted}' --after {next}");
        if limit != SEARCH_LIMIT {
            let _ = write!(text, " --limit {limit}");
        }
        text.push('\n');
    }
    let memory = report.memory;
    let notes = memory.map(render::memory_note).into_iter();
    let mut output = printer.print(SEARCH, Ok(render::said(value, text)));
    for note in notes {
        output.stderr.extend(format!("{note}\n").into_bytes());
    }
    output
}
