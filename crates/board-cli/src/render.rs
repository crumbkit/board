//! Turning reports into what `board` prints, in crumbkit's formats: text
//! for people, or one JSON document whose `schema` is
//! `board/<command>-report/v2` (docs/architecture.md, Commands).

use std::fmt;
use std::fmt::Write as _;

use board_model::{Code, Fields, Location, LogicalPath, Refusal};
use board_ops::MAX_LIMIT;
use board_ops::{
    Budget, CheckReport, Checked, Detail, EnableReport, Fallback, InitReport, Listed, LsReport,
    MountUpdateReport, PutReport, QueryReport, source_name,
};
use board_plugin::PluginName;
use board_tree::{Status, Synced};
use clap::error::{ContextKind, ContextValue, ErrorKind};
use crumbkit::help::Leaf;
use crumbkit::{Document, Listing, Prose};
use serde_json::{Map, Value, json};

use crate::report::Output;
use kit::{Flagged, Note, Refused, Said, Whole};
pub(crate) use kit::{GUIDE, Printer};

mod effects;
mod get;
mod kit;
mod page;
mod plugins;
mod refs;
mod ui;

pub(crate) use effects::{answer, decide};
pub(crate) use get::{GET_HELP, get};
use page::Shown;
pub(crate) use plugins::{CALL_REPORT, call, plugin_find, plugin_list};
pub(crate) use refs::{REFS_HELP, refs};
pub(crate) use ui::{serve, ui_kinds, ui_open, ui_template};

/// The leaf of each command, with the document it prints.
pub(crate) const LEAVES: &[Leaf] = &[
    Leaf::new(&["init"]).document(INIT),
    Leaf::new(&["check"]).document(CHECK),
    get::LEAF,
    Leaf::new(&["ls"]).document(LS),
    refs::LEAF,
    Leaf::new(&["put"]).document(PUT),
    Leaf::new(&["rm"]).document(PUT),
    Leaf::new(&["sync"]).document(SYNC),
    Leaf::new(&["mount", "update"]).document(MOUNT_UPDATE),
    plugins::LIST,
    plugins::FIND,
    Leaf::new(&["plugin", "enable"]).document(ENABLE),
    Leaf::new(&["type", "add"]).document(PUT),
    plugins::CALL,
    Leaf::new(&["query"]).document(QUERY),
    Leaf::new(&["decide"]).document(effects::DECIDE),
    Leaf::new(&["answer"]).document(effects::ANSWER),
    ui::LEAVES[0],
    ui::LEAVES[1],
    ui::LEAVES[2],
    ui::LEAVES[3],
];

/// The bytes of stdout one run prints before crumbkit cuts a listing or
/// leaves the remaining paths of `get` or `refs` out.
pub(crate) const BUDGET: usize = 64 << 20;

/// A command whose answer is a few lines of text, and a `data` object.
type Answer = Prose<Said, Refused>;

const fn prose(name: &'static str, description: &'static str) -> Document<Answer> {
    Document::new("board", name, 2, description)
}

const INIT: Document<Answer> = prose(
    "init-report",
    "Where the board is, and whether init made it.",
);
const PUT: Document<Answer> = prose(
    "put-report",
    "What each write wrote or removed, and its commit.",
);
const ENABLE: Document<Answer> = prose("plugin-enable-report", "What enabling a plugin wrote.");
const MOUNT_UPDATE: Document<Answer> =
    prose("mount-update-report", "Where a mount moved from and to.");
const SYNC: Document<Answer> = prose(
    "sync-report",
    "What a sync fetched, replayed and pushed, and where HEAD moved.",
);

/// `value` as prose that prints `text`.
fn said(value: Value, text: String) -> Answer {
    Prose::new(Said::new(value, text))
}

/// `check`: a line or an object per entry, degraded entries first, so a
/// limited listing never hides one behind healthy entries. Each degraded
/// entry reports a problem, so the run exits 1.
pub(crate) fn check(printer: Printer, report: &CheckReport, page: &crate::run::Page) -> Output {
    let mut entries: Vec<&Checked> = report.entries.iter().collect();
    entries.sort_by_key(|entry| entry.status == Status::Ok);
    let degraded = entries.iter().filter(|entry| entry.status != Status::Ok);
    let degraded = degraded.count();
    // Only the entries shown get a hint line (STANDARDS.md 23).
    let shown = page.limit().unwrap_or(usize::MAX);
    let mut hints = Vec::new();
    let rows = entries.into_iter().enumerate().map(|(at, entry)| {
        let path = named(&entry.path, entry.logical.as_ref(), entry.status);
        let why = entry.detail.as_ref().map(detail_text).unwrap_or_default();
        let line = format!("{} {path}{why}", Health(entry.status));
        let mut value = json!({
            "source": source_name(&entry.source),
            "path": entry.path,
            "logical": entry.logical.as_ref().map(ToString::to_string),
            "kind": entry.kind.as_str(),
        });
        status_fields(&mut value, entry.status);
        if let (Some(detail), Some(object)) = (&entry.detail, value.as_object_mut()) {
            object.extend(detail_fields(detail));
            if let Some(schema) = &entry.schema {
                object.insert("schema".to_owned(), json!(schema));
            }
        }
        if let (Some(hint), Some(object)) = (entry.hint(), value.as_object_mut()) {
            if at < shown {
                hints.push(format!("hint: {path}: {hint}"));
            }
            object.insert("hint".to_owned(), json!(hint));
        }
        Flagged(Whole::lined(value, line))
    });
    let listing = page.first_of(rows.collect()).described("entries");
    let total = report.entries.len();
    let summary = Said::new(
        json!({"degraded": degraded}),
        format!(
            "{total} {}, {degraded} degraded",
            if total == 1 { "entry" } else { "entries" }
        ),
    );
    let listing = listing.summary(summary);
    printer.noted(CHECK, listing, notes(report.memory, hints, &[]))
}

/// `check`'s document: each entry, degraded first, with the count of
/// degraded entries under `summary`.
const CHECK: Document<Listing<Flagged, Said>> = Document::new(
    "board",
    "check-report",
    2,
    "Every entry and whether it is healthy.",
)
.when_empty("no entries are at or beneath the path");

/// `put`: as text, one line per write naming its display path and new
/// blob, and the commit when one was made. A write left degraded gets a
/// note on stderr in either format, and `note` follows as text.
pub(crate) fn put(printer: Printer, report: &PutReport, note: Option<String>) -> Output {
    let notes = (report.writes.iter()).filter_map(|written| {
        let (code, detail) = written.degraded.as_ref()?;
        let (display, why) = (&written.display, detail_text(detail));
        let note = format!("note: {display} is written but degraded: {code} {display}{why}");
        Some(Note::Always(note))
    });
    let proposed = report.proposed.as_ref().map(|id| {
        Note::Text(format!(
            "next: the owner answers with `board answer {id} approve`, or refuse"
        ))
    });
    let answer = said(put_json(report), put_text(report));
    let notes = notes.chain(note.map(Note::Text)).chain(proposed);
    printer.noted(PUT, answer, notes.collect())
}

fn put_text(report: &PutReport) -> String {
    let mut text = String::new();
    for written in &report.writes {
        let _ = writeln!(text, "wrote {} {}", written.display, written.blob);
    }
    for removed in &report.removed {
        let _ = writeln!(text, "removed {} {}", removed.path, removed.blob);
    }
    if let Some(commit) = report.commit {
        let _ = writeln!(text, "committed {commit}");
    }
    if let Some(id) = &report.proposed {
        let _ = writeln!(text, "proposed {id}");
    }
    text
}

/// What a put wrote and its commit, as `put --json` and `plugin enable
/// --json` say it.
fn put_json(report: &PutReport) -> Value {
    let writes: Vec<Value> = report
        .writes
        .iter()
        .map(|written| {
            let degraded = written.degraded.as_ref().map(|(code, detail)| {
                let mut fields = detail_fields(detail);
                fields.insert("code".to_owned(), json!(code.as_str()));
                fields
            });
            json!({
                "location": location(&written.location, &written.display.to_string()),
                "display": written.display.to_string(),
                "previous": written.previous.map(|blob| blob.to_string()),
                "blob": written.blob.to_string(),
                "degraded": degraded,
            })
        })
        .collect();
    let mut value = json!({
        "writes": writes,
        "commit": report.commit.map(|commit| commit.to_string()),
    });
    if !report.removed.is_empty() {
        let removed = report.removed.iter();
        value["removed"] = removed
            .map(|removed| json!({"path": removed.path.to_string(), "blob": removed.blob.to_string()}))
            .collect();
    }
    if let Some(id) = &report.proposed {
        value["proposed"] = json!(id);
    }
    value
}

/// `plugin enable`: what it wrote, as `put` says it, or that nothing was
/// needed, with a note for each schema it replaced or removed.
pub(crate) fn plugin_enable(printer: Printer, report: &EnableReport) -> Output {
    let name = report.name.as_str();
    let text = if report.put.writes.is_empty() && report.removed.is_empty() {
        format!("{name} is already enabled\n")
    } else {
        format!("{}enabled {name}\n", put_text(&report.put))
    };
    let mut value = put_json(&report.put);
    value["plugin"] = json!(name);
    value["replaced"] = json!(report.replaced);
    value["removed"] = json!(report.removed);
    let notes = (report.replaced.iter()).map(|path| {
        format!(
            "note: replaced {path} with the schema the {name} plugin ships; records its old copy admitted may now be degraded, and `board check` lists them"
        )
    });
    let removed = (report.removed.iter())
        .map(|path| format!("note: removed {path}, which the {name} plugin no longer ships"));
    let notes = notes.chain(removed).map(Note::Always);
    printer.noted(ENABLE, said(value, text), notes.collect())
}

/// Why an entry fails, to follow its path, with a count of the other
/// failures.
fn detail_text(detail: &Detail) -> String {
    let mut text = format!(" {}", detail.reason);
    if detail.failures > 1 {
        let _ = write!(text, " (and {} more)", detail.failures - 1);
    }
    text
}

/// `pointer`, `reason` and `failures`, for a JSON report.
fn detail_fields(detail: &Detail) -> Map<String, Value> {
    let mut fields = Map::new();
    fields.insert("pointer".to_owned(), json!(detail.pointer));
    fields.insert("reason".to_owned(), json!(detail.reason));
    fields.insert("failures".to_owned(), json!(detail.failures));
    fields
}

/// The budget for a query's rows as crumbkit prints them: each row a
/// compact JSON array, its brackets and the comma or line feed after it,
/// and each value with the comma after it. crumbkit cuts the rows exactly;
/// this only stops the index fetching rows that cannot print.
pub(crate) fn rows_budget(bytes: usize) -> Budget {
    Budget {
        bytes,
        row: 3,
        value: |value| serde_json::to_string(value).map_or(0, |text| text.len()) + 1,
    }
}

/// `query`'s rows: as text, the columns, then one JSON array per row. A
/// note follows for each plugin that did not extract.
pub(crate) fn query(printer: Printer, report: &QueryReport) -> Output {
    let rows: Vec<Whole> = report
        .rows
        .iter()
        .map(|row| Whole::new(json!(row)))
        .collect();
    let listing = Listing::uncounted(rows, report.truncated)
        .bounded(MAX_LIMIT)
        .described("rows");
    let plugins: Vec<Value> = (report.plugins.iter())
        .map(|failed| json!({"plugin": failed.plugin, "code": failed.code, "message": failed.message}))
        .collect();
    let columns = json!(report.columns);
    let mut summary = json!({"columns": columns, "plugins": plugins});
    // Glue: crumbkit cannot tell a cut at the budget, which the index made,
    // from one at the limit, so Board names the budget's code itself.
    let over = Code::OutputOverBudget;
    let mut lines = Vec::new();
    if report.over_budget {
        summary["code"] = json!(over.as_str());
        let hint = over.hint(&Fields::default());
        lines.push(format!(
            "note: the next row would take the output past its budget ({over}); {hint}"
        ));
    }
    lines.extend((report.plugins.iter()).map(|failed| {
        let (plugin, code) = (&failed.plugin, &failed.code);
        format!(
            "note: plugin {plugin} did not extract: {code}: {}",
            failed.message
        )
    }));
    let listing = listing.summary(Said::new(summary, columns.to_string()));
    let fallbacks = &report.fallbacks;
    printer.noted(QUERY, listing, notes(report.memory, lines, fallbacks))
}

/// `query`'s document: the rows, with the columns and any plugin that did
/// not extract under `summary`.
const QUERY: Document<Listing<Whole, Said>> =
    Document::new("board", "query-report", 2, "The rows a query returned.");

/// The notes for a reader beside a listing: that the index was in memory,
/// each of `notes`, which say only what the document holds, then each
/// plugin in `fallbacks` whose database was a temporary file.
fn notes(memory: Option<Fallback>, notes: Vec<String>, fallbacks: &[PluginName]) -> Vec<Note> {
    let memory = memory.map(|fallback| Note::Always(memory_note(fallback)));
    let temporary = plugins::temporary(fallbacks).into_iter().map(Note::Always);
    (memory.into_iter())
        .chain(notes.into_iter().map(Note::Text))
        .chain(temporary)
        .collect()
}

/// The note that the index was in memory for this run, and why.
fn memory_note(fallback: Fallback) -> String {
    let why = match fallback {
        Fallback::NoCache => "there is no cache directory",
        Fallback::Busy => "another process held it past the wait",
        Fallback::Unusable => "it could not be used, even rebuilt",
    };
    format!("note: the index is in memory for this run: {why}")
}

/// `init`: as text, what was written, and on stderr what to run next.
pub(crate) fn init(printer: Printer, report: &InitReport) -> Output {
    let root = report.root.display();
    let text = if report.created {
        format!("created .board/board.json in {root}\n")
    } else {
        format!("{root} is already a board\n")
    };
    let value = json!({"root": report.root.to_string_lossy(), "created": report.created});
    let answer = said(value, text);
    let answer = if report.created {
        answer.note("next: run `board check`; it lists every file and says how to fix any it cannot read. Then commit .board/board.json")
    } else {
        answer
    };
    printer.print(INIT, Ok(answer))
}

pub(crate) fn ls(printer: Printer, report: &LsReport, page: &crate::run::Page) -> Output {
    let rows = report.listed.iter().map(|listed| match listed {
        Listed::Directory {
            name,
            logical: Some(logical),
        } => Whole::lined(
            json!({"name": name, "logical": logical.to_string(), "kind": "directory"}),
            format!("directory {logical}/"),
        ),
        Listed::Directory {
            name,
            logical: None,
        } => {
            let status = Status::Degraded(board_model::Code::PathMalformed);
            let mut value = json!({"name": name, "logical": null, "kind": "directory"});
            status_fields(&mut value, status);
            let line = format!(
                "directory {} {}/",
                Health(status),
                board_model::escaped(name)
            );
            Whole::lined(value, line)
        }
        Listed::Entry {
            name,
            logical,
            kind,
            status,
        } => {
            let mut value = json!({
                "name": name,
                "logical": logical.as_ref().map(ToString::to_string),
                "kind": kind.as_str(),
            });
            status_fields(&mut value, *status);
            let shown = logical
                .as_ref()
                .map_or_else(|| board_model::escaped(name), ToString::to_string);
            let line = format!("{} {} {shown}", kind.as_str(), Health(*status));
            Whole::lined(value, line)
        }
    });
    let display = report.display.to_string();
    let at = location(&report.location, &display);
    let summary = Said::new(json!({"location": at}), format!("{display}:"));
    let listing = page.first_of(rows.collect()).described("entries");
    let listing = listing.summary(summary);
    printer.noted(LS, listing, notes(report.memory, Vec::new(), &[]))
}

/// `ls`'s document: the entries, with the directory's `location` under
/// `summary`.
const LS: Document<Listing<Whole, Said>> =
    Document::new("board", "ls-report", 2, "The entries of one directory.")
        .when_empty("the directory is empty");

/// `mount update`: as text, what the mount moved from and to, or that it
/// was already there.
pub(crate) fn mount_update(printer: Printer, report: &MountUpdateReport) -> Output {
    let text = if report.wrote {
        format!("updated {} {} -> {}\n", report.path, report.old, report.new)
    } else {
        format!("{} is at {} already\n", report.path, report.new)
    };
    let value = json!({
        "path": report.path.to_string(),
        "old": report.old.to_string(),
        "new": report.new.to_string(),
        "wrote": report.wrote,
    });
    printer.print(MOUNT_UPDATE, Ok(said(value, text)))
}

/// `sync`: as text, a line for the fetch, each replayed commit, the push
/// and where HEAD moved.
pub(crate) fn sync(printer: Printer, report: &Synced) -> Output {
    let upstream = &report.upstream;
    let mut text = format!(
        "fetched {}/{} {}\n",
        upstream.remote, upstream.branch, report.fetched
    );
    for replayed in &report.replayed {
        let _ = match replayed.to {
            Some(to) => writeln!(text, "replayed {} -> {to}", replayed.from),
            None => writeln!(
                text,
                "dropped {}: the remote holds its changes",
                replayed.from
            ),
        };
    }
    if report.pushed {
        let after = match report.attempts {
            1 => String::new(),
            attempts => format!(" after {attempts} attempts"),
        };
        let _ = writeln!(text, "pushed {}{after}", report.new);
    }
    let _ = match report.old {
        Some(old) if old != report.new => writeln!(text, "moved HEAD {old} -> {}", report.new),
        Some(_) => writeln!(text, "HEAD is at {} already", report.new),
        None => writeln!(text, "checked out {}", report.new),
    };
    let replayed: Vec<Value> = report
        .replayed
        .iter()
        .map(|replayed| {
            json!({
                "from": replayed.from.to_string(),
                "to": replayed.to.map(|to| to.to_string()),
            })
        })
        .collect();
    let value = json!({
        "remote": upstream.remote,
        "branch": upstream.branch,
        "fetched": report.fetched.to_string(),
        "old": report.old.map(|old| old.to_string()),
        "new": report.new.to_string(),
        "replayed": replayed,
        "pushed": report.pushed,
        "attempts": report.attempts,
    });
    printer.print(SYNC, Ok(said(value, text)))
}

/// A refusal of a run that has no answer of its own to print.
pub(crate) fn refused(printer: Printer, refusal: Refusal) -> Output {
    printer.print(INIT, Err(refusal))
}

fn location(location: &Location, display: &str) -> Value {
    let mut value = json!({
        "repository": location.board.repository.to_string_lossy(),
        "source": source_name(&location.board.source),
        "path": location.path,
        "display": display,
    });
    if let Some(object) = value.as_object_mut() {
        let pointers: Vec<Value> = location
            .pointers
            .iter()
            .map(|pointer| {
                json!({
                    "repository": pointer.board.repository.to_string_lossy(),
                    "source": source_name(&pointer.board.source),
                    "path": pointer.path,
                })
            })
            .collect();
        object.insert("pointers".to_owned(), Value::Array(pointers));
    }
    value
}

/// How a text listing names an entry: by the logical path every command
/// takes, or by its file, escaped, when it has none or shares it with
/// another file.
fn named(path: &str, logical: Option<&LogicalPath>, status: Status) -> String {
    match logical {
        Some(logical) if status != Status::Degraded(board_model::Code::PathShadowed) => {
            logical.to_string()
        }
        _ => board_model::escaped(path),
    }
}

/// Whether an entry, a reference's target or a plugin is healthy. Text reads
/// `ok`, or `degraded` and the code of the problem. JSON reads `ok` or
/// `degraded`, and the row holds the code in its own `code` field.
struct Health(Status);

impl fmt::Display for Health {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Status::Ok => f.write_str("ok"),
            Status::Degraded(code) => write!(f, "degraded {code}"),
        }
    }
}

impl crumbkit::Shown for Health {
    fn json<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(match self.0 {
            Status::Ok => "ok",
            Status::Degraded(_) => "degraded",
        })
    }
}

crumbkit::shown!(Health);

/// Adds a JSON row's `status`, and its `code` when it is degraded.
fn status_fields(value: &mut Value, status: Status) {
    value["status"] = json!(Health(status));
    if let Status::Degraded(code) = status {
        value["code"] = json!(code.as_str());
    }
}

/// Board's wording of crumbkit's refusal of a command line given more than
/// `MAX_LIMIT` paths, which says to split them; `None` for any other refusal.
pub(crate) fn too_many_paths(unparsed: &crumbkit::Refused) -> Option<Output> {
    let error = unparsed.error()?;
    let (Some(ContextValue::String(argument)), Some(ContextValue::String(value))) = (
        error.get(ContextKind::InvalidArg),
        error.get(ContextKind::InvalidValue),
    ) else {
        return None;
    };
    if error.kind() != ErrorKind::TooManyValues {
        return None;
    }
    let message = format!(
        "{} was given more than {MAX_LIMIT} paths, and one run answers at most {MAX_LIMIT}",
        unparsed.path().join(" ")
    );
    let hint = format!("split the paths over several runs of at most {MAX_LIMIT} each");
    let mut refusal = Refusal::new(Code::CliUsage, &message, &Fields::default()).with_hint(hint);
    let name = argument.split(' ').next().unwrap_or(argument);
    refusal.fields.insert("argument".to_owned(), name.into());
    refusal
        .fields
        .insert("value".to_owned(), value.as_str().into());
    let printer = Printer {
        view: unparsed.output().clone(),
    };
    Some(refused(printer, refusal))
}
