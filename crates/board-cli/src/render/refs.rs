//! `refs`, of one path or of several.

use std::fmt::Write as _;

use board_model::{Code, Location, LogicalPath, Source};
use board_ops::{Fielded, Incoming, Missing, Outgoing, RefsError, RefsReport, source_name};
use board_tree::Status;
use serde_json::{Value, json};

use super::{
    Answers, Format, Shown, error_value, location, named, noted, refused, status_fields,
    status_text,
};
use crate::report::Output;

/// `board refs --help` after its options.
pub(crate) const REFS_HELP: &str = "\
Lines are `from <pointer> <target> <status>` for each reference the record
makes, then `to <path> <pointer>` for each record that reaches it.

With --field, each `from` line whose reference resolves adds
` target=<code>`, or ` target=-` for a healthy record, then
` <pointer>=<value>` for each field: the pointer as a JSON string, and the
target's value there as compact JSON, or `absent`. A target that is not a
record has no values, and neither has a target whose file changed after
the check: its code is board::refs::target_changed, and running refs again
reads it. A reference that resolves to nothing adds nothing.
A pointer is empty or starts with /, with ~0 for ~ and ~1 for / in a token.

With several paths, each is answered as it would be alone, in the order
given. The limit applies to each path, so a run prints at most the number of
paths times the limit reference lines, plus a note or error for each path.
Every line about a path names it, notes and errors included. A path that is
refused does not stop the others; the exit code is 1.
--json prints one board/refs-report/v1, for one path too, whose `answers`
hold each path's references or its board/error/v1, in order; with --field,
each `from` whose reference resolves has `values`, each a `pointer` with
its `value` or `absent`, and `target_code` when the target has a code.
One run takes at most 10000 paths. It stops answering once its output, as
text or JSON, reaches 64 MiB, and refuses each path after that as
board::refs::over_budget, to ask for in another run. A path that is not a
logical path, such as tasks/a, refuses the whole run as
board::usage::invalid with exit code 2.

Examples:
  board refs /projects/p
  board refs /projects/p --json
  board refs /tasks/a /tasks/b --all --json
  board refs /tasks/a --field /status --field /title
  board refs --missing /projects/p/notes/new.md --json";

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
                "{prefix}from {} {} {}{}",
                reference.pointer,
                reference.target,
                status_text(reference.status),
                fields_text(reference.fields.as_ref())
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
                if let Some(fielded) = &reference.fields {
                    fields_value(&mut value, fielded);
                }
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

/// `refs` of `paths`, each answered as it would be alone and limited on its
/// own, in order. Each path is answered only once the answers before it are
/// rendered, and only while the run has printed fewer than `budget` bytes;
/// every later path is `refs::over_budget`, and the first is always
/// answered. A check that could not run refuses the whole run. As text, a `from` line per reference
/// the record makes, with its status and any fields, and a `to` line per
/// record whose references reach it; with several paths, every line about
/// a path names it. As JSON, one `board/refs-report/v1` whose `answers`
/// hold, in the order asked, each path's references or its
/// `board/error/v1`. Exit 1 when any path is refused.
pub(crate) fn refs(
    format: Format,
    paths: &[LogicalPath],
    mut answers: impl Iterator<Item = Result<RefsReport, RefsError>>,
    limit: Option<usize>,
    budget: usize,
) -> Output {
    let several = paths.len() > 1;
    let mut run = Answers::open(format, "board/refs-report/v1", budget);
    let mut memory = None;
    for (at, path) in paths.iter().enumerate() {
        let answer = if run.fits(at) {
            match answers.next() {
                Some(Err(error @ RefsError::Check { .. })) => {
                    return refused(format, &error.refusal());
                }
                Some(answer) => answer.map_err(|error| error.refusal()),
                None => break,
            }
        } else {
            Err(run.over_budget(Code::RefsOverBudget, "answered", path))
        };
        let name = |text: &str| {
            if several {
                named_lines(text, path)
            } else {
                text.to_owned()
            }
        };
        match &answer {
            Ok(report) => memory = memory.or(report.memory),
            Err(refusal) => run.refused(refusal),
        }
        match (format, &answer) {
            (Format::Json, answer) => run.json(
                &answer
                    .as_ref()
                    .map_or_else(error_value, |report| Cut::of(report, limit).value(report)),
            ),
            (Format::Text, Ok(report)) => {
                let cut = Cut::of(report, limit);
                let line = if several {
                    format!("{path} ")
                } else {
                    String::new()
                };
                run.output.stdout.extend(cut.lines(&line).bytes());
                run.output.stderr.extend(name(&cut.note(report)).bytes());
            }
            (Format::Text, Err(refusal)) => {
                let alone = refused(Format::Text, refusal).stderr;
                run.output
                    .stderr
                    .extend(name(&String::from_utf8_lossy(&alone)).bytes());
            }
        }
    }
    noted(run.close(), memory, None)
}

/// What a `from` line adds for the fields a run asked for, when its
/// reference resolves: ` target=<code>`, or ` target=-` for a healthy
/// record, then ` <pointer>=<value>` for each field, with the pointer as a
/// JSON string and the value as compact JSON or `absent`.
fn fields_text(fielded: Option<&Fielded>) -> String {
    let mut text = String::new();
    let Some(fielded) = fielded else {
        return text;
    };
    let code = fielded.code.map_or("-", Code::as_str);
    let _ = write!(text, " target={code}");
    for (pointer, value) in fielded.values.iter().flatten() {
        let value = value
            .as_ref()
            .map_or_else(|| "absent".to_owned(), Value::to_string);
        let _ = write!(text, " {}={value}", Value::from(pointer.as_str()));
    }
    text
}

/// Adds to a `from` object the target's `values`, in the order asked, each
/// `{"pointer", "value"}` or `{"pointer", "absent": true}`, when the
/// target is a record that could be read, and its `target_code` when it
/// has one.
fn fields_value(value: &mut Value, fielded: &Fielded) {
    if let Some(values) = &fielded.values {
        let values: Vec<Value> = values
            .iter()
            .map(|(pointer, found)| match found {
                Some(found) => json!({"pointer": pointer, "value": found}),
                None => json!({"pointer": pointer, "absent": true}),
            })
            .collect();
        value["values"] = json!(values);
    }
    if let Some(code) = fielded.code {
        value["target_code"] = json!(code.as_str());
    }
}

/// Each line of `text` with `path` after its label, such as `error: ` or
/// `line 3: `, or before it when it has none.
pub(super) fn named_lines(text: &str, path: &LogicalPath) -> String {
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
