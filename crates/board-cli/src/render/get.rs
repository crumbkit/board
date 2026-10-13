//! `get`, of one path or of several.

use board_model::{Code, LogicalPath, Object, Refusal};
use board_ops::{GetReport, ReadError};
use data_encoding::BASE64;
use serde_json::{Value, json};

use super::refs::named_lines;
use super::{Answers, Format, error_value, location, refused};
use crate::report::Output;

/// `board get --help` after its options.
pub(crate) const GET_HELP: &str = "\
One path prints the file's stored bytes alone. Several print each file after
a `==> <path> <==` line, as `head` does, in the order given; --json is the
exact form. A refused path does not stop the others; its error lines name
it, and the exit code is 1.

--json always prints one board/get-report/v1, whose `answers` hold, in
order, each file or its board/error/v1, told apart by `$type`. A file has
its location, its blob, its bytes as `text` (`base64` if not UTF-8), and
its `record` or a `code` saying why not, such as board::record::not_a_record
for a markdown file.

One run takes at most 10000 paths; more is board::usage::invalid. It stops
reading at 64 MiB of output and refuses each later path as
board::get::over_budget; ask for those in another run.

Examples:
  board get /projects/p
  board get /projects/p --json
  board get /tasks/a /tasks/b /tasks/c --json
  board get /README.md";

/// `get` of `paths`. Each file is read only once the answers before it are
/// rendered, and only while the run has printed fewer than `budget` bytes;
/// every later path is `get::over_budget`, and the first is always read.
/// When the board could not be read at all, every path is answered with
/// that one error. As text, one path prints its file's stored bytes alone;
/// several print each after a `==> <path> <==` line, as `head` does with
/// several files, and name the path on every line of a refusal. As JSON,
/// one `board/get-report/v1` whose `answers` hold, in the order asked, each
/// file or its `board/error/v1`. Exit 1 when any path is refused.
pub(crate) fn get(
    format: Format,
    paths: &[LogicalPath],
    answers: Result<impl Iterator<Item = Result<GetReport, ReadError>>, ReadError>,
    budget: usize,
) -> Output {
    let several = paths.len() > 1;
    let mut run = Answers::open(format, "board/get-report/v1", budget);
    let (mut each, unread) = match answers {
        Ok(each) => (Some(each), None),
        Err(error) => (None, Some(error.refusal("get"))),
    };
    for (at, path) in paths.iter().enumerate() {
        let answer = if let Some(refusal) = &unread {
            Err(refusal.clone())
        } else if !run.fits(at) {
            Err(run.over_budget(Code::GetOverBudget, "read", path))
        } else {
            match each.as_mut().and_then(Iterator::next) {
                Some(Ok(report)) => Ok(report),
                Some(Err(error)) => Err(error.refusal("get")),
                None => break,
            }
        };
        if let Err(refusal) = &answer {
            run.refused(refusal);
        }
        match format {
            Format::Text => text(&mut run.output, several.then_some(path), &answer),
            Format::Json => run.json(&answer.as_ref().map_or_else(error_value, value)),
        }
    }
    run.close()
}

/// One answer as text: the file's bytes, after its header when `named`
/// gives its path, or the refusal's lines on stderr, each naming the path.
fn text(output: &mut Output, named: Option<&LogicalPath>, answer: &Result<GetReport, Refusal>) {
    match (answer, named) {
        (Ok(report), Some(path)) => {
            let gap = if output.stdout.is_empty() { "" } else { "\n" };
            output
                .stdout
                .extend(format!("{gap}==> {path} <==\n").bytes());
            output.stdout.extend_from_slice(&report.bytes);
        }
        (Ok(report), None) => output.stdout.extend_from_slice(&report.bytes),
        (Err(refusal), named) => {
            let alone = refused(Format::Text, refusal).stderr;
            let alone = String::from_utf8_lossy(&alone);
            let lines = named.map_or_else(|| alone.to_string(), |path| named_lines(&alone, path));
            output.stderr.extend(lines.bytes());
        }
    }
}

/// One file in a `board/get-report/v1`: where it is, its blob, its stored
/// bytes as `text` when they are UTF-8 and as `base64` when they are not,
/// and its `record` or the `code` that says why it is not one.
fn value(report: &GetReport) -> Value {
    let blob = match report.location.object {
        Object::Blob(blob) => Some(blob.to_string()),
        Object::Directory => None,
    };
    let mut value = json!({
        "location": location(&report.location, &report.display.to_string()),
        "blob": blob,
    });
    match std::str::from_utf8(&report.bytes) {
        Ok(text) => value["text"] = json!(text),
        Err(_) => value["base64"] = json!(BASE64.encode(&report.bytes)),
    }
    match &report.record {
        Ok(record) => value["record"] = record.clone(),
        Err(refusal) => value["code"] = json!(refusal.code.as_str()),
    }
    value
}
