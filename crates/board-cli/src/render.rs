//! Turning reports into the bytes `board` prints: text for people, or one
//! JSON document tagged `board/<command>-report/v1` (docs/architecture.md,
//! Commands).

use std::fmt::Display;
use std::fmt::Write as _;

use board_model::{Code, Location, Source};
use board_ops::{CheckReport, Fallback, GetReport, Listed, LsReport, source_name};
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
    let Some(fallback) = report.memory else {
        return output;
    };
    let why = match fallback {
        Fallback::NoCache => "there is no cache directory",
        Fallback::Busy => "another process held it past the wait",
        Fallback::Unusable => "it could not be used, even rebuilt",
    };
    Output {
        stderr: format!("note: the index is in memory for this run: {why}\n").into_bytes(),
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
