//! Turning reports into the bytes `board` prints: text for people, or one
//! JSON document tagged `board/<command>-report/v1` (docs/architecture.md,
//! Commands).

use std::fmt::Display;
use std::fmt::Write as _;

use board_model::{Code, Location, LogicalPath, Source};
use board_ops::{
    CheckReport, Fallback, GetReport, Listed, LsReport, MountUpdateReport, PutError, PutReport,
    QueryReport, RefsReport, source_name,
};
use board_tree::Status;
use serde_json::{Value, json};

use crate::report::Output;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Format {
    Text,
    Json,
}

pub(crate) fn check(format: Format, report: &CheckReport) -> Output {
    let code = u8::from(report.degraded());
    let output = match format {
        Format::Text => {
            let mut text = String::new();
            for entry in &report.entries {
                // A mounted entry's path means nothing in the root board.
                let shown = match (&entry.source, &entry.logical) {
                    (Source::Commit(_), Some(logical)) => logical.to_string(),
                    _ => entry.path.clone(),
                };
                let _ = writeln!(text, "{} {shown}", status_text(entry.status));
            }
            Output {
                stdout: text.into_bytes(),
                code,
                ..Output::default()
            }
        }
        Format::Json => {
            let entries: Vec<Value> = report
                .entries
                .iter()
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
                &json!({
                    "$type": "board/check-report/v1",
                    "degraded": report.degraded(),
                    "entries": entries,
                }),
                code,
            )
        }
    };
    noted(output, report.memory, false)
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
            document(
                &json!({
                    "$type": "board/put-report/v1",
                    "writes": writes,
                    "commit": report.commit.map(|commit| commit.to_string()),
                }),
                0,
            )
        }
    }
}

/// `refs`: as text, a `from` line per reference the object makes, with its
/// status, and a `to` line per record whose references reach it.
pub(crate) fn refs(format: Format, report: &RefsReport) -> Output {
    let named = |source: &Source, path: &str, logical: Option<&LogicalPath>| match (source, logical)
    {
        (Source::Commit(_), Some(logical)) => logical.to_string(),
        _ => path.to_owned(),
    };
    let output = match format {
        Format::Text => {
            let mut text = String::new();
            for reference in &report.from {
                let _ = writeln!(
                    text,
                    "from {} {} {}",
                    reference.pointer,
                    reference.target,
                    status_text(reference.status)
                );
            }
            for incoming in &report.to {
                let name = named(&incoming.source, &incoming.path, incoming.logical.as_ref());
                for pointer in &incoming.pointers {
                    let _ = writeln!(text, "to {name} {pointer}");
                }
            }
            Output {
                stdout: text.into_bytes(),
                ..Output::default()
            }
        }
        Format::Json => {
            let from: Vec<Value> = report
                .from
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
            let to: Vec<Value> = report
                .to
                .iter()
                .map(|incoming| {
                    json!({
                        "source": source_name(&incoming.source),
                        "path": incoming.path,
                        "logical": incoming.logical.as_ref().map(ToString::to_string),
                        "pointers": incoming.pointers,
                    })
                })
                .collect();
            document(
                &json!({
                    "$type": "board/refs-report/v1",
                    "location": location(&report.location, &report.display.to_string()),
                    "from": from,
                    "to": to,
                }),
                0,
            )
        }
    };
    noted(output, report.memory, false)
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
        Format::Json => document(
            &json!({
                "$type": "board/query-report/v1",
                "columns": report.columns,
                "rows": report.rows,
                "truncated": report.truncated,
            }),
            0,
        ),
    };
    noted(
        output,
        report.memory,
        report.truncated && format == Format::Text,
    )
}

/// `output` with a line on stderr for each thing a reader should know:
/// that the index was in memory, or that rows were left out.
fn noted(output: Output, memory: Option<Fallback>, truncated: bool) -> Output {
    let mut stderr = String::new();
    if let Some(fallback) = memory {
        let why = match fallback {
            Fallback::NoCache => "there is no cache directory",
            Fallback::Busy => "another process held it past the wait",
            Fallback::Unusable => "it could not be used, even rebuilt",
        };
        let _ = writeln!(stderr, "note: the index is in memory for this run: {why}");
    }
    if truncated {
        let _ = writeln!(stderr, "note: more rows matched; raise --limit to see them");
    }
    Output {
        stderr: stderr.into_bytes(),
        ..output
    }
}

pub(crate) fn get(format: Format, report: &GetReport) -> Output {
    match format {
        Format::Text => Output {
            stdout: report.bytes.clone(),
            ..Output::default()
        },
        Format::Json => document(
            &json!({
                "$type": "board/get-report/v1",
                "location": location(&report.location, &report.display.to_string()),
                "record": report.record,
            }),
            0,
        ),
    }
}

pub(crate) fn ls(format: Format, report: &LsReport) -> Output {
    match format {
        Format::Text => {
            let mut text = String::new();
            for listed in &report.listed {
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
                ..Output::default()
            }
        }
        Format::Json => {
            let entries: Vec<Value> = report
                .listed
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
                &json!({
                    "$type": "board/ls-report/v1",
                    "location": location(&report.location, &report.display.to_string()),
                    "entries": entries,
                }),
                0,
            )
        }
    }
}

/// A failure: `error: <code>: <message>` on stderr, or a `board/error/v1`
/// document on stdout. Either way the exit code is 1.
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
        return self::error(format, error.code(), error);
    };
    match format {
        Format::Text => {
            let mut text = format!("error: {code}: the batch is refused\n");
            for refusal in refusals {
                let _ = writeln!(
                    text,
                    "line {}: {}: {}",
                    refusal.line, refusal.code, refusal.message
                );
            }
            Output {
                stderr: text.into_bytes(),
                code: 1,
                ..Output::default()
            }
        }
        Format::Json => {
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
            document(
                &json!({
                    "$type": "board/error/v1",
                    "code": code.as_str(),
                    "message": "the batch is refused",
                    "fields": {"refusals": refusals},
                }),
                1,
            )
        }
    }
}

pub(crate) fn error(format: Format, code: Code, message: &dyn Display) -> Output {
    match format {
        Format::Text => Output {
            stderr: format!("error: {code}: {message}\n").into_bytes(),
            code: 1,
            ..Output::default()
        },
        Format::Json => document(
            &json!({
                "$type": "board/error/v1",
                "code": code.as_str(),
                "message": message.to_string(),
            }),
            1,
        ),
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
