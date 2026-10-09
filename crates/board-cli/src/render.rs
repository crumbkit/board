//! Turning reports into the bytes `board` prints: text for people, or one
//! JSON document tagged `board/<command>-report/v1` (docs/architecture.md,
//! Commands).

use std::fmt::Display;
use std::fmt::Write as _;

use board_model::{Code, Fields, Location, LogicalPath, Object, Refusal, RefusalCode, Source};
use board_ops::{
    CheckReport, Checked, Detail, EnableReport, Fallback, GetReport, InitReport, Listed, LsReport,
    MAX_LIMIT, Missing, MountUpdateReport, PutError, PutReport, QueryReport, RefsReport,
    source_name,
};
use board_tree::Status;
use serde_json::{Map, Value, json};

use crate::report::Output;

mod page;
mod plugins;

use page::Shown;
pub(crate) use plugins::{call, call_error, plugin_find, plugin_list, temporary};

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
            for entry in entries {
                let path = named(&entry.path, entry.logical.as_ref(), entry.status);
                let why = entry.detail.as_ref().map(detail_text).unwrap_or_default();
                let _ = writeln!(text, "{} {path}{why}", status_text(entry.status));
            }
            Output {
                stdout: text.into_bytes(),
                stderr: shown
                    .note(
                        &format!("entries, {degraded} of them degraded"),
                        "no entries are at or beneath the path",
                    )
                    .into_bytes(),
                code,
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
                "note: {display} is written but degraded: {code}{why}"
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

/// Where and why an entry fails, to follow its path or code: ` at
/// <pointer>: <reason>`, without `at` when the pointer is unknown or the
/// record's root, and with a count of the other failures.
fn detail_text(detail: &Detail) -> String {
    let mut text = match detail.pointer.as_deref() {
        None | Some("") => format!(": {}", detail.reason),
        Some(pointer) => format!(" at {pointer}: {}", detail.reason),
    };
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

/// `refs`: as text, a `from` line per reference the object makes, with its
/// status, and a `to` line per record whose references reach it.
pub(crate) fn refs(format: Format, report: &RefsReport, limit: Option<usize>) -> Output {
    // One listing, a line per reference: those made, then each pointer that
    // reaches the record, so a cut can fall within one incoming record.
    let reaching: usize = report
        .to
        .iter()
        .map(|incoming| incoming.pointers.len())
        .sum();
    let shown = Shown::of(report.from.len() + reaching, limit);
    let from = &report.from[..shown.shown.min(report.from.len())];
    let mut budget = shown.shown - from.len();
    let to: Vec<(&board_ops::Incoming, &[String])> = report
        .to
        .iter()
        .map_while(|incoming| {
            let take = incoming.pointers.len().min(budget);
            budget -= take;
            (take > 0).then(|| (incoming, &incoming.pointers[..take]))
        })
        .collect();
    let output = match format {
        Format::Text => {
            let mut text = String::new();
            for reference in from {
                let _ = writeln!(
                    text,
                    "from {} {} {}",
                    reference.pointer,
                    reference.target,
                    status_text(reference.status)
                );
            }
            for (incoming, pointers) in &to {
                let name = named(&incoming.path, incoming.logical.as_ref(), Status::Ok);
                for pointer in *pointers {
                    let _ = writeln!(text, "to {name} {pointer}");
                }
            }
            Output {
                stdout: text.into_bytes(),
                stderr: shown
                    .note(
                        "references from and to the record",
                        &report.missing.as_ref().map_or_else(
                            || "the record makes no references, and none reach it".to_owned(),
                            |missing| missing_note(&report.location, missing),
                        ),
                    )
                    .into_bytes(),
                ..Output::default()
            }
        }
        Format::Json => {
            let from: Vec<Value> = from
                .iter()
                .map(|reference| {
                    let mut value = json!({
                        "pointer": reference.pointer,
                        "target": reference.target.to_string(),
                        "location": reference.location.as_ref().map(|location| {
                            let display = location
                                .display_path()
                                .map_or_else(|| reference.target.to_string(), |path| path.to_string());
                            self::location(location, &display)
                        }),
                    });
                    status_fields(&mut value, reference.status);
                    value
                })
                .collect();
            let to: Vec<Value> = to
                .iter()
                .map(|(incoming, pointers)| {
                    json!({
                        "source": source_name(&incoming.source),
                        "path": incoming.path,
                        "logical": incoming.logical.as_ref().map(ToString::to_string),
                        "pointers": pointers,
                    })
                })
                .collect();
            document(
                &shown.mark(json!({
                    "$type": "board/refs-report/v1",
                    "location": location(&report.location, &report.display.to_string()),
                    "missing": report.missing.as_ref().map(|missing| json!({
                        "segments": missing.segments,
                        "path": missing.path,
                        "display": missing.display.to_string(),
                    })),
                    "from": from,
                    "to": to,
                })),
                0,
            )
        }
    };
    noted(output, report.memory, None)
}

/// Where a file at a path that names nothing would be, naming the mounted
/// board when it is in one.
fn missing_note(location: &Location, missing: &Missing) -> String {
    let mounted = match &location.board.source {
        Source::WorkingTree => String::new(),
        Source::Commit(commit) => format!(
            " in the board mounted from {} at commit {commit}",
            location.board.repository.display()
        ),
    };
    format!(
        "nothing is there yet; a file there would be {}, at {}{mounted}",
        missing.display, missing.path
    )
}

/// `query`'s rows: as text, one JSON array per line, the columns first.
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
            document(
                &json!({
                    "$type": "board/query-report/v1",
                    "columns": report.columns,
                    "rows": report.rows,
                    "truncated": report.truncated,
                    "plugins": plugins,
                }),
                0,
            )
        }
    };
    let rows = (report.truncated && format == Format::Text).then_some(report.rows.len());
    let mut output = noted(output, report.memory, rows);
    if format == Format::Text {
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
            "note: showing the first {shown} rows, and more matched; --limit <n> (at most {MAX_LIMIT}) shows more"
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

pub(crate) fn get(format: Format, report: &GetReport) -> Output {
    match format {
        Format::Text => Output {
            stdout: report.bytes.clone(),
            ..Output::default()
        },
        // The report carries the parsed record, so a file that is not one
        // has no report, and is refused as the report says.
        Format::Json => match &report.record {
            Ok(record) => {
                let blob = match report.location.object {
                    Object::Blob(blob) => Some(blob),
                    Object::Directory => None,
                };
                document(
                    &json!({
                        "$type": "board/get-report/v1",
                        "location": location(&report.location, &report.display.to_string()),
                        "blob": blob.map(|blob| blob.to_string()),
                        "record": record,
                    }),
                    0,
                )
            }
            Err(refusal) => refused(format, refusal),
        },
    }
}

pub(crate) fn ls(format: Format, report: &LsReport, limit: Option<usize>) -> Output {
    let shown = Shown::of(report.listed.len(), limit);
    let listed = &report.listed[..shown.shown];
    match format {
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
    }
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

/// A refused put. A refused batch lists each failing line: as text one line
/// each after the error, and in JSON as `fields.refusals`.
pub(crate) fn put_error(format: Format, error: &PutError) -> Output {
    let PutError::Batch { code, refusals } = error else {
        return self::error(format, error.code(), error, &error.fields());
    };
    let message = "the batch is refused";
    if format == Format::Json {
        let refusals: Vec<Value> = refusals
            .iter()
            .map(|refusal| {
                json!({
                    "line": refusal.line,
                    "code": refusal.code.as_str(),
                    "message": refusal.message,
                })
            })
            .collect();
        let fields = Map::from_iter([("refusals".to_owned(), Value::Array(refusals))]);
        return refused(
            format,
            &Refusal {
                fields,
                ..Refusal::new(*code, message, &Fields::default()).with_hint(BATCH_HINT)
            },
        );
    }
    let mut text = format!("error: {code}: {message}\n");
    for refusal in refusals {
        let _ = writeln!(
            text,
            "line {}: {}: {}",
            refusal.line, refusal.code, refusal.message
        );
    }
    let _ = writeln!(text, "hint: {BATCH_HINT}");
    Output {
        stderr: text.into_bytes(),
        code: 1,
        ..Output::default()
    }
}

/// What to do about a refused batch, whatever refused its lines.
const BATCH_HINT: &str =
    "fix each line listed, then send the whole batch again; nothing was written";

/// A failure with the hint for its code, filled in from `fields`, which
/// JSON carries as `fields`.
pub(crate) fn error(format: Format, code: Code, message: &dyn Display, fields: &Fields) -> Output {
    refused(format, &Refusal::new(code, message, fields))
}

/// A refusal: as text, `error: <code>: <message>`, then `hint: <hint>` and
/// the end of a plugin's stderr, on stderr; as JSON, a `board/error/v1`
/// document on stdout, with `fields` when there are any. The exit code is 2
/// for a refused command line, else 1.
pub(crate) fn refused(format: Format, refusal: &Refusal) -> Output {
    let code = if refusal.code == RefusalCode::Board(Code::UsageInvalid) {
        2
    } else {
        1
    };
    match format {
        Format::Text => {
            let mut text = format!("error: {}: {}\n", refusal.code.as_str(), refusal.message);
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
                ..Output::default()
            }
        }
        Format::Json => {
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
            document(&value, code)
        }
    }
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
        "source": source(&location.board.source),
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
                    "source": source(&pointer.board.source),
                    "path": pointer.path,
                })
            })
            .collect();
        object.insert("pointers".to_owned(), Value::Array(pointers));
    }
    value
}

fn source(source: &Source) -> Value {
    match source {
        Source::WorkingTree => json!("working-tree"),
        Source::Commit(commit) => json!({ "commit": commit.to_string() }),
    }
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
