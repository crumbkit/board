//! `get`, of one path or of several.

use board_model::{Code, LogicalPath, Object};
use board_ops::{GetReport, ReadError};
use crumbkit::help::Leaf;
use crumbkit::{Batch, Bytes, Document, Headed, Row};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;

use super::kit::{Printer, Refused};
use super::location;
use crate::report::Output;

/// `board get --help` after its options.
pub(crate) const GET_HELP: &str = "\
One path prints the file's bytes. Several print each after a `==> <path> <==`
line, in order. A refused path does not stop the others; the exit code is 1.

--json prints board/get-report/v2: `data` holds each file read and `failures`
each path refused, by `index`. A file has its location, blob, `text` (or
`base64`), and its `record` or a `code` saying why not.

At most 10000 paths. Past 64 MiB of output, one board::output::over_budget
names the paths `left_out`.

Without --fetch, get prints a board/attachment/v1 record and never reaches
the store that holds its bytes.

Examples:
  board get /projects/p
  board get /projects/p --json
  board get /tasks/a /tasks/b /tasks/c --json
  board get /README.md";

/// `get` of `paths`. Each file is read only while crumbkit has room for it
/// in the run's budget of stdout, and the first always; crumbkit answers
/// the paths after it with one `board::output::over_budget`. When the board
/// could not be read at all, every path is answered with that one error.
/// Exit 1 when any path is refused.
pub(crate) fn get(
    printer: &mut Printer,
    paths: &[LogicalPath],
    answers: Result<impl Iterator<Item = Result<GetReport, ReadError>>, ReadError>,
) -> Output {
    let mut room = printer.view.room(GET);
    let mut batch = Batch::new(&["get"], "paths", paths);
    let (mut each, unread) = match answers {
        Ok(each) => (Some(each), None),
        Err(error) => (None, Some(error.refusal("get"))),
    };
    for _ in paths {
        let answer = if let Some(refusal) = &unread {
            Err(refusal.clone())
        } else if !room.has_room(&batch) {
            break;
        } else {
            match each.as_mut().and_then(Iterator::next) {
                Some(Ok(report)) => Ok(FileAnswer::new(report)),
                Some(Err(error)) => Err(error.refusal("get")),
                None => break,
            }
        };
        batch.push(answer.map_err(Refused::new));
    }
    let mut output = printer.print(GET, Ok(batch));
    if output.code != 0 && output.error.is_none() {
        output.error = Some(Code::OutputOverBudget.as_str().to_owned());
    }
    output
}

/// `get`'s document: each file read, and each path refused, and the text
/// prints each file's bytes, after a header when there are several.
const GET: Document<Batch<FileAnswer, Refused>> =
    Document::new("board", "get-report", 2, "Each file asked for, or why not.")
        .always_print_first()
        .headed();

pub(super) const LEAF: Leaf = Leaf::new(&["get"]).document(GET);

/// One file in a `board/get-report/v2`: where it is, its blob, its stored
/// bytes as `text` when they are UTF-8 and as `base64` when they are not,
/// and its `record` or the `code` that says why it is not one.
#[derive(Serialize, JsonSchema)]
struct FileAnswer {
    location: Value,
    blob: Option<String>,
    #[serde(flatten)]
    content: Bytes,
    #[serde(skip_serializing_if = "Option::is_none")]
    record: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<String>,
}

impl FileAnswer {
    fn new(report: GetReport) -> Self {
        let blob = match report.location.object {
            Object::Blob(blob) => Some(blob.to_string()),
            Object::Directory => None,
        };
        let (record, code) = match report.record {
            Ok(record) => (Some(record), None),
            Err(refusal) => (None, Some(refusal.code.as_str().to_owned())),
        };
        Self {
            location: location(&report.location, &report.display.to_string()),
            blob,
            content: Bytes::new(report.bytes),
            record,
            code,
        }
    }
}

impl Row for FileAnswer {
    const COLUMNS: &'static [&'static str] = &[];
}

/// Board's text prints the stored bytes of every file, a record's too.
impl Headed for FileAnswer {
    fn bytes(&self) -> Option<&Bytes> {
        Some(&self.content)
    }
}
