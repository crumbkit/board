//! Turning reports into the bytes `board` prints: text for people, or one
//! JSON document tagged `board/<command>-report/v1` (docs/architecture.md,
//! Commands).

use std::fmt::Display;
use std::fmt::Write as _;

use board_model::{Code, Fields, Location, LogicalPath, Object, Source};
use board_ops::{
    CheckReport, Checked, EnableReport, Fallback, GetReport, InitReport, Known, Listed, LsReport,
    MAX_LIMIT, MountUpdateReport, PutError, PutReport, QueryError, QueryReport, RefsReport,
    source_name,
};
use board_tree::Status;
use serde_json::{Map, Value, json};

use crate::report::Output;

mod hint;
mod page;
mod plugins;

pub(crate) use hint::hint;
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
                let _ = writeln!(text, "{} {path}", status_text(entry.status));
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
/// blob, and the commit when one was made.
pub(crate) fn put(format: Format, report: &PutReport) -> Output {
    match format {
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
    }
}

/// What a put wrote and its commit, as `put --json` and `plugin enable
/// --json` say it.
fn put_json(report: &PutReport) -> Value {
    let writes: Vec<Value> = report
        .writes
        .iter()
        .map(|written| {
            json!({
                "location": location(&written.location, &written.display.to_string()),
                "display": written.display.to_string(),
                "previous": written.previous.map(|blob| blob.to_string()),
                "blob": written.blob.to_string(),
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
                        "the record makes no references, and none reach it",
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
                    "from": from,
                    "to": to,
                })),
                0,
            )
        }
    };
    noted(output, report.memory, None)
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

/// A query that could not run. When it names a missing table or column, the
/// hint lists the names there are.
pub(crate) fn query_error(format: Format, error: &QueryError) -> Output {
    let QueryError::Unknown { known, .. } = error else {
        return self::error(format, error.code(), error, &Fields::default());
    };
    let hint = match known {
        Known::Tables(tables) => format!(
            "the tables and views are {}; `board query --help` describes them",
            tables.join(", ")
        ),
        Known::Columns(tables) if tables.is_empty() => "run `board query \"SELECT name, sql FROM sqlite_master\"` for every table and its columns".to_owned(),
        Known::Columns(tables) => {
            let tables: Vec<String> = tables
                .iter()
                .map(|(table, columns)| format!("{table}: {}", columns.join(", ")))
                .collect();
            format!("the columns are {}", tables.join("; "))
        }
    };
    failed(
        format,
        error.code().as_str(),
        &error.to_string(),
        Some(&hint),
        Map::new(),
    )
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
        // has no report, and is refused with the code that says why.
        Format::Json => {
            let blob = match report.location.object {
                Object::Blob(blob) => Some(blob),
                Object::Directory => None,
            };
            match &report.record {
                Ok(record) => document(
                    &json!({
                        "$type": "board/get-report/v1",
                        "location": location(&report.location, &report.display.to_string()),
                        "blob": blob.map(|blob| blob.to_string()),
                        "record": record,
                    }),
                    0,
                ),
                Err(code) => error(
                    format,
                    *code,
                    &format_args!("{} is not a record", report.display),
                    &Fields {
                        blob,
                        ..Fields::at(&report.display)
                    },
                ),
            }
        }
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
        return failed(format, code.as_str(), message, Some(BATCH_HINT), fields);
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
    failed(
        format,
        code.as_str(),
        &message.to_string(),
        Some(&hint(code, fields)),
        json_fields(fields),
    )
}

/// The values in `fields` that are known, by their `board/error/v1` names.
pub(crate) fn json_fields(fields: &Fields) -> Map<String, Value> {
    let known = [
        ("path", fields.path.as_ref().map(ToString::to_string)),
        ("blob", fields.blob.as_ref().map(ToString::to_string)),
        ("pointer", fields.pointer.clone()),
        ("type", fields.record_type.as_ref().map(ToString::to_string)),
        ("schema", fields.schema.clone()),
        ("plugin", fields.plugin.clone()),
        ("capability", fields.capability.clone()),
    ];
    known
        .into_iter()
        .filter_map(|(name, value)| Some((name.to_owned(), Value::String(value?))))
        .collect()
}

/// A failure with a hint that knows more than its code: as text,
/// `error: <code>: <message>` and then `hint: <hint>` on stderr; as JSON, a
/// `board/error/v1` document on stdout, with `fields` when there are any.
/// Either way the exit code is 1.
pub(crate) fn failed(
    format: Format,
    code: &str,
    message: &str,
    hint: Option<&str>,
    fields: Map<String, Value>,
) -> Output {
    match format {
        Format::Text => {
            let mut text = format!("error: {code}: {message}\n");
            if let Some(hint) = hint {
                let _ = writeln!(text, "hint: {hint}");
            }
            Output {
                stderr: text.into_bytes(),
                code: 1,
                ..Output::default()
            }
        }
        Format::Json => {
            let mut value = json!({
                "$type": "board/error/v1",
                "code": code,
                "message": message,
            });
            if let Some(hint) = hint {
                value["hint"] = json!(hint);
            }
            if !fields.is_empty() {
                value["fields"] = Value::Object(fields);
            }
            document(&value, 1)
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
