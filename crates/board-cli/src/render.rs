//! Turning reports into the bytes `board` prints: text for people, or one
//! JSON document tagged `board/<command>-report/v1` (docs/architecture.md,
//! Commands).

use std::fmt::Display;
use std::fmt::Write as _;

use board_model::{Code, Fields, Location, LogicalPath, Refusal, RefusalCode};
use board_ops::{
    Budget, CheckReport, Checked, Detail, EnableReport, Fallback, InitReport, Listed, LsReport,
    MAX_LIMIT, MountUpdateReport, PutReport, QueryReport, source_name,
};
use board_tree::Status;
use serde_json::{Map, Value, json};

use crate::report::Output;

mod get;
mod page;
mod plugins;
mod refs;

pub(crate) use get::{GET_HELP, get};
use page::Shown;
pub(crate) use plugins::{call, plugin_find, plugin_list, temporary};
pub(crate) use refs::{REFS_HELP, refs};

/// The bytes one `get` or `refs` run prints, in the format it prints,
/// before it leaves the remaining paths unanswered as `over_budget`.
pub(crate) const BUDGET: usize = 64 << 20;

/// A run's answers to several paths, printed in order while the run has
/// printed fewer than `budget` bytes, as text or, as JSON, inside one
/// report whose `answers` are written one at a time. The output is held
/// until exit, so it is at most `budget`, one answer, and the refusals of
/// the paths left.
struct Answers {
    output: Output,
    format: Format,
    budget: usize,
    written: usize,
}

impl Answers {
    fn open(format: Format, kind: &str, budget: usize) -> Self {
        let mut output = Output::default();
        if format == Format::Json {
            let open = format!("{{\n  \"$type\": \"{kind}\",\n  \"answers\": [");
            output.stdout.extend(open.bytes());
        }
        Self {
            output,
            format,
            budget,
            written: 0,
        }
    }

    /// Whether the answer to the path at `at` is printed, or is
    /// `over_budget`. The first always is.
    fn fits(&self, at: usize) -> bool {
        at == 0 || self.output.stdout.len() + self.output.stderr.len() < self.budget
    }

    /// The answer to `path` once the run has printed its budget: it was
    /// not `done`, such as `read`.
    fn over_budget(&self, code: Code, done: &str, path: &LogicalPath) -> Refusal {
        let budget = self.budget;
        let message =
            format!("{path} was not {done}: this run already printed its budget of {budget} bytes");
        Refusal::new(code, message, &Fields::at(path))
    }

    /// Notes `refusal`, whose code becomes the run's when it is the first.
    fn refused(&mut self, refusal: &Refusal) {
        (self.output.error).get_or_insert_with(|| refusal.code.as_str().to_owned());
    }

    /// Writes `value` as the next of the JSON report's `answers`.
    fn json(&mut self, value: &Value) {
        let pretty = serde_json::to_string_pretty(value).unwrap_or_default();
        let separator = if self.written == 0 {
            "\n    "
        } else {
            ",\n    "
        };
        self.written += 1;
        self.output.stdout.extend(separator.bytes());
        self.output
            .stdout
            .extend(pretty.replace('\n', "\n    ").bytes());
    }

    /// The output, with exit 1 when any path was refused.
    fn close(mut self) -> Output {
        if self.format == Format::Json {
            self.output.stdout.extend_from_slice(b"\n  ]\n}\n");
        }
        self.output.code = u8::from(self.output.error.is_some());
        self.output
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Format {
    Text,
    Json,
}

/// `check`: a line or an object per entry, degraded entries first, so a
/// limited listing never hides one behind healthy entries.
pub(crate) fn check(format: Format, report: &CheckReport, limit: Option<usize>) -> Output {
    let code = u8::from(report.degraded());
    let mut entries: Vec<&Checked> = report.entries.iter().collect();
    entries.sort_by_key(|entry| entry.status == Status::Ok);
    let shown = Shown::of(entries.len(), limit);
    entries.truncate(shown.shown);
    let degraded = report
        .entries
        .iter()
        .filter(|entry| entry.status != Status::Ok)
        .count();
    let output = match format {
        Format::Text => {
            let mut text = String::new();
            let mut hints = String::new();
            for entry in entries {
                let path = named(&entry.path, entry.logical.as_ref(), entry.status);
                let why = entry.detail.as_ref().map(detail_text).unwrap_or_default();
                let _ = writeln!(text, "{} {path}{why}", status_text(entry.status));
                if let Some(hint) = entry.hint() {
                    let _ = writeln!(hints, "hint: {path}: {hint}");
                }
            }
            hints.push_str(&shown.note(
                &format!("entries, {degraded} of them degraded"),
                "no entries are at or beneath the path",
            ));
            Output {
                stdout: text.into_bytes(),
                stderr: hints.into_bytes(),
                code,
                error: None,
            }
        }
        Format::Json => {
            let entries: Vec<Value> = entries
                .into_iter()
                .map(|entry| {
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
                        object.insert("hint".to_owned(), json!(hint));
                    }
                    value
                })
                .collect();
            document(
                &shown.mark(json!({
                    "$type": "board/check-report/v1",
                    "degraded": report.degraded(),
                    "degraded_total": degraded,
                    "entries": entries,
                })),
                code,
            )
        }
    };
    noted(output, report.memory, None)
}

/// `put`: as text, one line per write naming its display path and new
/// blob, and the commit when one was made. A write left degraded gets a
/// note on stderr in either format.
pub(crate) fn put(format: Format, report: &PutReport) -> Output {
    let mut notes = String::new();
    for written in &report.writes {
        if let Some((code, detail)) = &written.degraded {
            let why = detail_text(detail);
            let display = &written.display;
            let _ = writeln!(
                notes,
                "note: {display} is written but degraded: {code} {display}{why}"
            );
        }
    }
    let output = match format {
        Format::Text => {
            let mut text = String::new();
            for written in &report.writes {
                let _ = writeln!(text, "wrote {} {}", written.display, written.blob);
            }
            if let Some(commit) = report.commit {
                let _ = writeln!(text, "committed {commit}");
            }
            Output {
                stdout: text.into_bytes(),
                ..Output::default()
            }
        }
        Format::Json => {
            let mut report = put_json(report);
            report["$type"] = json!("board/put-report/v1");
            document(&report, 0)
        }
    };
    Output {
        stderr: notes.into_bytes(),
        ..output
    }
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
    json!({
        "writes": writes,
        "commit": report.commit.map(|commit| commit.to_string()),
    })
}

/// `plugin enable`: what it wrote, as `put` says it, or that nothing was
/// needed.
pub(crate) fn plugin_enable(format: Format, report: &EnableReport) -> Output {
    let name = report.name.as_str();
    match format {
        Format::Text if report.put.writes.is_empty() => Output {
            stdout: format!("{name} is already enabled\n").into_bytes(),
            ..Output::default()
        },
        Format::Text => {
            let mut output = put(format, &report.put);
            output
                .stdout
                .extend_from_slice(format!("enabled {name}\n").as_bytes());
            output
        }
        Format::Json => {
            let mut json = put_json(&report.put);
            json["$type"] = json!("board/plugin-enable-report/v1");
            json["plugin"] = json!(name);
            document(&json, 0)
        }
    }
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

/// Counts the bytes written to it, and keeps none.
#[derive(Default)]
struct Counted {
    bytes: usize,
}

impl std::io::Write for Counted {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.bytes += buf.len();
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The budget for a query's rows, `bytes` as `format` prints them: as
/// text, each row's line, `[` and a newline, and each value and the `,` or
/// `]` after it; as JSON, everything `rows` adds to `[]`. That is
/// `,\n    [\n` and `\n    ]` for each row, each value indented by 6
/// spaces with the `,\n` or `\n` after it, and 2 bytes more, as the first
/// row has no comma and the list closes with `\n  ]`.
pub(crate) fn rows_budget(format: Format, bytes: usize) -> Budget {
    let (row, framing, value): (usize, usize, fn(&Value) -> usize) = match format {
        Format::Text => (2, 0, |value| printed(value) + 1),
        Format::Json => (12, 2, |value| printed(value) + 8),
    };
    Budget {
        bytes: bytes.saturating_sub(framing),
        row,
        value,
    }
}

/// The bytes `value` takes as compact JSON. A value from `SQLite` is never
/// an array or an object, so pretty JSON prints it the same.
fn printed(value: &Value) -> usize {
    let mut counted = Counted::default();
    let _ = serde_json::to_writer(&mut counted, value);
    counted.bytes
}

/// `query`'s rows: as text, one JSON array per line, the columns first.
/// Rows left out at the budget are noted with `query::over_budget`.
pub(crate) fn query(format: Format, report: &QueryReport) -> Output {
    let output = match format {
        Format::Text => {
            let mut text = format!("{}\n", json!(report.columns));
            for row in &report.rows {
                let _ = writeln!(text, "{}", json!(row));
            }
            Output {
                stdout: text.into_bytes(),
                ..Output::default()
            }
        }
        Format::Json => {
            let plugins: Vec<Value> = report
                .plugins
                .iter()
                .map(|failed| {
                    json!({
                        "plugin": failed.plugin,
                        "code": failed.code,
                        "message": failed.message,
                    })
                })
                .collect();
            let mut value = json!({
                "$type": "board/query-report/v1",
                "columns": report.columns,
                "rows": report.rows,
                "truncated": report.truncated,
                "plugins": plugins,
            });
            if report.over_budget {
                value["code"] = json!(Code::QueryOverBudget.as_str());
            }
            document(&value, 0)
        }
    };
    let cut = report.truncated && !report.over_budget && format == Format::Text;
    let mut output = noted(output, report.memory, cut.then_some(report.rows.len()));
    if format == Format::Text {
        if report.over_budget {
            let note = format!(
                "note: showing the first {} rows, and more matched, but the next would take the output past its budget ({}); {}\n",
                report.rows.len(),
                Code::QueryOverBudget,
                Code::QueryOverBudget.hint(&Fields::default())
            );
            output.stderr.extend_from_slice(note.as_bytes());
        }
        for failed in &report.plugins {
            output.stderr.extend_from_slice(
                format!(
                    "note: plugin {} did not extract: {}: {}\n",
                    failed.plugin, failed.code, failed.message
                )
                .as_bytes(),
            );
        }
    }
    temporary(output, &report.fallbacks)
}

/// `output` with a line on stderr before its own notes for each thing a
/// reader should know: that the index was in memory, or that rows were
/// left out of `shown`.
fn noted(output: Output, memory: Option<Fallback>, rows: Option<usize>) -> Output {
    let mut stderr = String::new();
    if let Some(fallback) = memory {
        let why = match fallback {
            Fallback::NoCache => "there is no cache directory",
            Fallback::Busy => "another process held it past the wait",
            Fallback::Unusable => "it could not be used, even rebuilt",
        };
        let _ = writeln!(stderr, "note: the index is in memory for this run: {why}");
    }
    if let Some(shown) = rows {
        let _ = writeln!(
            stderr,
            "note: showing the first {shown} rows, and more matched; --limit <n> (at most {MAX_LIMIT}) or --all shows more"
        );
    }
    let mut stderr = stderr.into_bytes();
    stderr.extend_from_slice(&output.stderr);
    Output { stderr, ..output }
}

/// `init`: as text, what was written and what to run next.
pub(crate) fn init(format: Format, report: &InitReport) -> Output {
    let root = report.root.display();
    match format {
        Format::Text => {
            let text = if report.created {
                format!(
                    "created .board/board.json in {root}\nnext: run `board check`; it lists every file and says how to fix any it cannot read. Then commit .board/board.json\n"
                )
            } else {
                format!("{root} is already a board\n")
            };
            Output {
                stdout: text.into_bytes(),
                ..Output::default()
            }
        }
        Format::Json => document(
            &json!({
                "$type": "board/init-report/v1",
                "root": report.root.to_string_lossy(),
                "created": report.created,
            }),
            0,
        ),
    }
}

pub(crate) fn ls(format: Format, report: &LsReport, limit: Option<usize>) -> Output {
    let shown = Shown::of(report.listed.len(), limit);
    let listed = &report.listed[..shown.shown];
    let output = match format {
        Format::Text => {
            let mut text = String::new();
            for listed in listed {
                let _ = match listed {
                    Listed::Directory { logical, .. } => writeln!(text, "directory {logical}/"),
                    Listed::Entry {
                        name,
                        logical,
                        kind,
                        status,
                    } => {
                        let shown = logical
                            .as_ref()
                            .map_or_else(|| name.clone(), ToString::to_string);
                        writeln!(text, "{} {} {shown}", kind.as_str(), status_text(*status))
                    }
                };
            }
            Output {
                stdout: text.into_bytes(),
                stderr: shown.note("entries", "the directory is empty").into_bytes(),
                ..Output::default()
            }
        }
        Format::Json => {
            let entries: Vec<Value> = listed
                .iter()
                .map(|listed| match listed {
                    Listed::Directory { name, logical } => json!({
                        "name": name,
                        "logical": logical.to_string(),
                        "kind": "directory",
                    }),
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
                        value
                    }
                })
                .collect();
            document(
                &shown.mark(json!({
                    "$type": "board/ls-report/v1",
                    "location": location(&report.location, &report.display.to_string()),
                    "entries": entries,
                })),
                0,
            )
        }
    };
    noted(output, report.memory, None)
}

/// `mount update`: as text, what the mount moved from and to, or that it
/// was already there.
pub(crate) fn mount_update(format: Format, report: &MountUpdateReport) -> Output {
    match format {
        Format::Text => {
            let text = if report.wrote {
                format!("updated {} {} -> {}\n", report.path, report.old, report.new)
            } else {
                format!("{} is at {} already\n", report.path, report.new)
            };
            Output {
                stdout: text.into_bytes(),
                ..Output::default()
            }
        }
        Format::Json => document(
            &json!({
                "$type": "board/mount-update-report/v1",
                "path": report.path.to_string(),
                "old": report.old.to_string(),
                "new": report.new.to_string(),
                "wrote": report.wrote,
            }),
            0,
        ),
    }
}

/// The refusal of a run of `command` given `count` paths, when that is more
/// than the `most` one run answers.
pub(crate) fn too_many(format: Format, command: &str, count: usize, most: usize) -> Option<Output> {
    let message = format!("{command} was given {count} paths, and one run answers at most {most}");
    let hint = format!("split the paths over several runs of at most {most} each");
    (count > most).then(|| {
        let refusal = Refusal::new(Code::UsageInvalid, message, &Fields::default());
        refused(format, &refusal.with_hint(hint))
    })
}

/// A failure with the hint for its code, filled in from `fields`, which
/// JSON carries as `fields`.
pub(crate) fn error(format: Format, code: Code, message: &dyn Display, fields: &Fields) -> Output {
    refused(format, &Refusal::new(code, message, fields))
}

/// A refusal: as text, `error: <code>: <message>`, a line for each refused
/// line of a batch, then `hint: <hint>` and the end of a plugin's stderr, on
/// stderr; as JSON, a `board/error/v1`
/// document on stdout, with `fields` when there are any. The exit code is 2
/// for a refused command line, else 1.
pub(crate) fn refused(format: Format, refusal: &Refusal) -> Output {
    let error = Some(refusal.code.as_str().to_owned());
    let code = if refusal.code == RefusalCode::Board(Code::UsageInvalid) {
        2
    } else {
        1
    };
    match format {
        Format::Text => {
            let mut text = format!("error: {}: {}\n", refusal.code.as_str(), refusal.message);
            for line in &refusal.lines {
                let _ = writeln!(text, "line {}: {}: {}", line.line, line.code, line.message);
            }
            if let Some(hint) = &refusal.hint {
                let _ = writeln!(text, "hint: {hint}");
            }
            let mut stderr = text.into_bytes();
            if !refusal.stderr.is_empty() {
                stderr.extend_from_slice(&refusal.stderr);
                if !refusal.stderr.ends_with(b"\n") {
                    stderr.push(b'\n');
                }
            }
            Output {
                stderr,
                code,
                error,
                ..Output::default()
            }
        }
        Format::Json => Output {
            error,
            ..document(&error_value(refusal), code)
        },
    }
}

/// The `board/error/v1` document of a refusal.
fn error_value(refusal: &Refusal) -> Value {
    let mut value = json!({
        "$type": "board/error/v1",
        "code": refusal.code.as_str(),
        "message": refusal.message,
    });
    if let Some(hint) = &refusal.hint {
        value["hint"] = json!(hint);
    }
    if !refusal.fields.is_empty() {
        value["fields"] = Value::Object(refusal.fields.clone());
    }
    value
}

fn document(value: &Value, code: u8) -> Output {
    let mut stdout = serde_json::to_vec_pretty(value).unwrap_or_default();
    stdout.push(b'\n');
    Output {
        stdout,
        code,
        ..Output::default()
    }
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
/// takes, or by its file when it has none or shares it with another file.
fn named(path: &str, logical: Option<&LogicalPath>, status: Status) -> String {
    match logical {
        Some(logical) if status != Status::Degraded(Code::PathShadowed) => logical.to_string(),
        _ => path.to_owned(),
    }
}

fn status_text(status: Status) -> String {
    match status {
        Status::Ok => "ok".to_owned(),
        Status::Degraded(code) => format!("degraded {code}"),
    }
}

fn status_fields(value: &mut Value, status: Status) {
    if let Some(object) = value.as_object_mut() {
        match status {
            Status::Ok => {
                object.insert("status".to_owned(), json!("ok"));
            }
            Status::Degraded(code) => {
                object.insert("status".to_owned(), json!("degraded"));
                object.insert("code".to_owned(), json!(code.as_str()));
            }
        }
    }
}
