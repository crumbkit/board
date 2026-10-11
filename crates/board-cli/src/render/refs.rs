//! `refs`, of one path or of several.

use std::fmt::Write as _;

use board_model::{Location, LogicalPath, Refusal, Source};
use board_ops::{Incoming, Missing, Outgoing, RefsReport, source_name};
use board_tree::Status;
use serde_json::{Value, json};

use super::{
    Format, Shown, document, error_value, location, named, noted, refused, status_fields,
    status_text,
};
use crate::report::Output;

/// What a limit shows of one path's answer. It is one listing, a line per
/// reference: those made, then each pointer that reaches the record, so a
/// cut can fall within one incoming record.
struct Cut<'a> {
    shown: Shown,
    from: &'a [Outgoing],
    to: Vec<(&'a Incoming, &'a [String])>,
}

impl<'a> Cut<'a> {
    fn of(report: &'a RefsReport, limit: Option<usize>) -> Self {
        let reaching: usize = report
            .to
            .iter()
            .map(|incoming| incoming.pointers.len())
            .sum();
        let shown = Shown::of(report.from.len() + reaching, limit);
        let from = &report.from[..shown.shown.min(report.from.len())];
        let mut budget = shown.shown - from.len();
        let to = report
            .to
            .iter()
            .map_while(|incoming| {
                let take = incoming.pointers.len().min(budget);
                budget -= take;
                (take > 0).then(|| (incoming, &incoming.pointers[..take]))
            })
            .collect();
        Self { shown, from, to }
    }

    /// A `from` line per reference shown, with its status, then a `to` line
    /// per pointer shown that reaches the record, each after `prefix`.
    fn lines(&self, prefix: &str) -> String {
        let mut text = String::new();
        for reference in self.from {
            let _ = writeln!(
                text,
                "{prefix}from {} {} {}",
                reference.pointer,
                reference.target,
                status_text(reference.status)
            );
        }
        for (incoming, pointers) in &self.to {
            let name = named(&incoming.path, incoming.logical.as_ref(), Status::Ok);
            for pointer in *pointers {
                let _ = writeln!(text, "{prefix}to {name} {pointer}");
            }
        }
        text
    }

    /// The note for stderr: what was left out, that there is nothing, or
    /// where a file at a path that names nothing would be.
    fn note(&self, report: &RefsReport) -> String {
        self.shown.note(
            "references from and to the record",
            &report.missing.as_ref().map_or_else(
                || "the record makes no references, and none reach it".to_owned(),
                |missing| missing_note(&report.location, missing),
            ),
        )
    }

    /// One path's answer in a `board/refs-report/v1`.
    fn value(&self, report: &RefsReport) -> Value {
        let from: Vec<Value> = self
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
                        super::location(location, &display)
                    }),
                });
                status_fields(&mut value, reference.status);
                value
            })
            .collect();
        let to: Vec<Value> = self
            .to
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
        self.shown.mark(json!({
            "location": location(&report.location, &report.display.to_string()),
            "missing": report.missing.as_ref().map(|missing| json!({
                "segments": missing.segments,
                "path": missing.path,
                "display": missing.display.to_string(),
            })),
            "from": from,
            "to": to,
        }))
    }
}

/// `refs` of one path or several, each answered as it would be alone and
/// limited on its own. As text, a `from` line per reference the record
/// makes, with its status, and a `to` line per record whose references
/// reach it; with several paths, every line about a path names it. As
/// JSON, one `board/refs-report/v1` whose `answers` hold, in the order
/// asked, each path's references or its `board/error/v1`.
/// Exit 1 when any path is refused.
pub(crate) fn refs(
    format: Format,
    answers: &[(&LogicalPath, Result<RefsReport, Refusal>)],
    limit: Option<usize>,
) -> Output {
    let several = answers.len() > 1;
    let (mut stdout, mut stderr, mut values) = (String::new(), Vec::new(), Vec::new());
    let mut error = None;
    for (path, answer) in answers {
        let name = |text: &str| {
            if several {
                named_lines(text, path)
            } else {
                text.to_owned()
            }
        };
        match answer {
            Ok(report) => {
                let cut = Cut::of(report, limit);
                let line = if several {
                    format!("{path} ")
                } else {
                    String::new()
                };
                stdout.push_str(&cut.lines(&line));
                stderr.extend(name(&cut.note(report)).bytes());
                values.push(cut.value(report));
            }
            Err(refusal) => {
                error.get_or_insert_with(|| refusal.code.as_str().to_owned());
                let alone = refused(Format::Text, refusal).stderr;
                stderr.extend(name(&String::from_utf8_lossy(&alone)).bytes());
                values.push(error_value(refusal));
            }
        }
    }
    let code = u8::from(error.is_some());
    let output = match format {
        Format::Text => Output {
            stdout: stdout.into_bytes(),
            stderr,
            code,
            error,
        },
        Format::Json => Output {
            error,
            ..document(
                &json!({"$type": "board/refs-report/v1", "answers": values}),
                code,
            )
        },
    };
    let memory = answers
        .iter()
        .find_map(|(_, answer)| answer.as_ref().ok().and_then(|report| report.memory));
    noted(output, memory, None)
}

/// Each line of `text` with `path` after its label, such as `error: ` or
/// `line 3: `, or before it when it has none.
fn named_lines(text: &str, path: &LogicalPath) -> String {
    let mut named = String::new();
    for line in text.lines() {
        let _ = match line.split_once(": ") {
            Some((label, rest)) => writeln!(named, "{label}: {path}: {rest}"),
            None => writeln!(named, "{path}: {line}"),
        };
    }
    named
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_line_names_the_path_after_its_label_or_before_a_line_without_one() {
        let path: LogicalPath = "/tasks/a".parse().unwrap();
        assert_eq!(
            named_lines("error: code: why\nline 3: code: what\nplain\n", &path),
            "error: /tasks/a: code: why\nline 3: /tasks/a: code: what\n/tasks/a: plain\n"
        );
        assert_eq!(named_lines("", &path), "");
    }
}
