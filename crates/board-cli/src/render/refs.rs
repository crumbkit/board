//! `refs`, of one path or of several.

use std::fmt::Write as _;

use board_model::{Code, Location, LogicalPath, Source};
use board_ops::{Fielded, Incoming, Missing, Outgoing, RefsError, RefsReport, source_name};
use board_tree::Status;
use crumbkit::help::{Leaf, Rename};
use crumbkit::{Batch, Bytes, Document, Headed, Row};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{Value, json};

use super::kit::{Note, Printer, Refused};
use super::{Shown, location, named, status_fields, status_text};
use crate::report::Output;

/// `board refs --help` after its options.
pub(crate) const REFS_HELP: &str = "\
Lines are `from <pointer> <target> <status>` for each reference the record
makes, then `to <path> <pointer>` for each record that reaches it. Several
paths print each after a `==> <path> <==` line; --limit applies to each. A
refused path does not stop the others; the exit code is 1.

--field adds ` target=<code>`, or ` target=-` when healthy, then
` <pointer>=<value>` to each `from` line that resolves: the pointer as a
JSON string, the value as compact JSON or `absent`. For
board::refs::target_changed, run refs again.

--json prints board/refs-report/v2: `data` holds each path's references and
`failures` each path refused, by `index`; --field adds `values` and
`target_code`.

At most 10000 paths. Past 64 MiB of output, one board::refs::over_budget
names the paths `left_out`.

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
    /// per pointer shown that reaches the record.
    fn lines(&self) -> String {
        let mut text = String::new();
        for reference in self.from {
            let _ = writeln!(
                text,
                "from {} {} {}{}",
                reference.pointer,
                reference.target,
                status_text(reference.status),
                fields_text(reference.fields.as_ref())
            );
        }
        for (incoming, pointers) in &self.to {
            let name = named(&incoming.path, incoming.logical.as_ref(), Status::Ok);
            for pointer in *pointers {
                let _ = writeln!(text, "to {name} {pointer}");
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

    /// One path's answer in a `board/refs-report/v2`.
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
/// own, in order. Each path is answered only while the run has room for it
/// in its budget of stdout, and the first always; crumbkit answers the paths
/// after it with one `refs::over_budget`. A check that could not run
/// refuses the whole run. As text, a `from` line per reference the record
/// makes, with its status and any fields, and a `to` line per record whose
/// references reach it, after a header when there are several paths. Exit
/// 1 when any path is refused.
pub(crate) fn refs(
    printer: &mut Printer,
    paths: &[LogicalPath],
    mut answers: impl Iterator<Item = Result<RefsReport, RefsError>>,
    limit: Option<usize>,
) -> Output {
    let mut room = printer.view.room(REFS);
    let mut batch = Batch::new(&["refs"], "paths", paths);
    let (mut memory, mut notes) = (None, Vec::new());
    for path in paths {
        if !room.has_room(&batch) {
            break;
        }
        let answer = match answers.next() {
            Some(Err(error @ RefsError::Check { .. })) => {
                return printer.print(REFS, Err(error.refusal()));
            }
            Some(answer) => answer.map_err(|error| Refused::new(error.refusal())),
            None => break,
        };
        batch.push(answer.map(|report| {
            memory = memory.or(report.memory);
            let cut = Cut::of(&report, limit);
            let note = cut.note(&report);
            let note = note.trim_end_matches('\n');
            match note.strip_prefix("note: ") {
                Some(rest) if paths.len() > 1 => {
                    notes.push(Note::Text(format!("note: {path}: {rest}")));
                }
                _ if !note.is_empty() => notes.push(Note::Text(note.to_owned())),
                _ => {}
            }
            Answered {
                value: cut.value(&report),
                text: Bytes::new(cut.lines().into_bytes()),
            }
        }));
    }
    notes.extend(memory.map(|fallback| Note::Always(super::memory_note(fallback))));
    let mut output = printer.noted(REFS, batch, notes);
    if output.code != 0 && output.error.is_none() {
        output.error = Some(Code::RefsOverBudget.as_str().to_owned());
    }
    output
}

/// One path's answer: its references as JSON, and as text lines.
#[derive(Serialize, JsonSchema)]
#[serde(transparent)]
struct Answered {
    value: Value,
    #[serde(skip)]
    text: Bytes,
}

impl Row for Answered {
    const COLUMNS: &'static [&'static str] = &[];
}

impl Headed for Answered {
    fn bytes(&self) -> Option<&Bytes> {
        Some(&self.text)
    }
}

/// `refs`'s document: each path's references, and each path refused.
const REFS: Document<Batch<Answered, Refused>> = Document::new(
    "board",
    "refs-report",
    2,
    "The references of each path asked for.",
)
.always_print_first()
.headed();

const OVER_BUDGET: crumbkit::Code = crumbkit::Code::new("board", "refs", "over_budget");

/// `refs`'s leaf, which prints Board's code for the paths the budget left
/// out.
pub(super) const LEAF: Leaf = Leaf::new(&["refs"]).document(REFS).codes(&[Rename::new(
    &crumbkit::codes::OUTPUT_OVER_BUDGET,
    &OVER_BUDGET,
)]);

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
