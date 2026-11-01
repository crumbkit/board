//! The commands that read stdin: `put`, `call` and `type add`.

use std::ffi::OsString;
use std::io::Read;

use board_model::{Code, LogicalPath, MAX_RECORD_BYTES, RecordType, Refusal};
use board_ops::{Board, Expect};
use board_plugin::Target;
use clap::Subcommand;
use serde_json::{Map, Value};

use crate::config::Context;
use crate::render::{self, Printer};
use crate::report::Output;
use crate::run::{Prefix, Putting, plain, plugins, read, read_folder};

#[derive(Debug, Subcommand)]
pub(crate) enum TypeCommand {
    /// Add a type: compile the JSON Schema on stdin and write it to .board/types/.
    #[command(after_help = "\
An equal schema already there is left as it is. One that differs only in
its top-level `x-board-folder` replaces it. Any other difference is refused,
because a changed type is a new version. The write is committed unless
.board/board.json sets `commit_on_put` to false.

Examples:
  board type add notes/note/v1 < note.schema.json
  board type add notes/note/v2 --json < note.schema.json")]
    Add {
        /// The type, `<namespace>/<kind>/v<n>`, such as notes/note/v1; never in the `board` namespace.
        #[arg(value_name = "TYPE")]
        record_type: RecordType,
    },
}

/// Where a put writes: each line of a batch, a new name in a folder or in
/// the folder the record's type declares, or a path.
enum At {
    Batch,
    New(LogicalPath),
    Declared,
    Path(LogicalPath),
}

/// `put`: a batch on stdin, or the record on stdin at a path or a new name
/// in a folder, or with `file`, stdin as a file, read whole, since a file
/// past the record bound may go to the store.
pub(crate) fn put(
    board: &Board,
    printer: Printer,
    prefix: Option<&Prefix>,
    putting: Putting,
    stdin: &mut dyn Read,
) -> Output {
    let Putting {
        path,
        new,
        var,
        expect,
        batch,
        file,
        wait,
    } = putting;
    let at = match (path, new) {
        _ if batch => Ok(At::Batch),
        (Some(folder), true) => read_folder(prefix, &folder).map(At::New),
        (Some(path), false) => read(prefix, &path, "put").map(At::Path),
        (None, true) => Ok(At::Declared),
        (None, false) => unreachable!("clap requires a path, --new or --batch"),
    };
    let at = match at {
        Ok(at) => at,
        Err(refusal) => return render::refused(printer, *refusal),
    };
    let read = match at {
        At::Batch => Ok(Vec::new()),
        At::Path(_) if file => {
            let mut bytes = Vec::new();
            stdin.read_to_end(&mut bytes).map(|_| bytes)
        }
        At::Path(_) | At::New(_) | At::Declared => bounded(stdin),
    };
    let bytes = match read.map_err(|error| unreadable(Code::RepoUnreadable, &error)) {
        Ok(bytes) => bytes,
        Err(refusal) => return render::refused(printer, refusal),
    };
    let expect = expect.unwrap_or(Expect::Any);
    let written = match &at {
        At::Batch => board.put_batch_waiting(&mut std::io::BufReader::new(stdin), wait),
        At::New(folder) => board.put_new_waiting(folder, &bytes, wait),
        At::Declared => board.put_declared_waiting(&var.into_iter().collect(), &bytes, wait),
        At::Path(path) if file => board.put_file_waiting(path, expect, &bytes, wait),
        At::Path(path) => board.put_waiting(path, expect, &bytes, wait),
    };
    match written {
        Ok(report) => render::put(printer, &report, None),
        Err(error) => render::refused(printer, error.refusal()),
    }
}

/// `call`: the target, or the capability alone, with the input as flags or,
/// when no flag is given, on `stdin` unless the call reads none. A terminal
/// on stdin, or nothing read from it, is no input.
pub(crate) fn call(
    board: &Board,
    context: &Context,
    printer: Printer,
    target: &Target,
    flags: Option<Map<String, Value>>,
    stdin: Option<&mut dyn Read>,
) -> Output {
    let read = match (flags.filter(|flags| !flags.is_empty()), stdin) {
        (Some(flags), _) => Ok(Value::Object(flags).to_string().into_bytes()),
        (None, Some(stdin)) if !context.terminal => bounded(stdin),
        (None, _) => Ok(Vec::new()),
    };
    let input = match read.map_err(|error| unreadable(Code::PluginBadInput, &error)) {
        Ok(input) if input.trim_ascii().is_empty() => b"{}".to_vec(),
        Ok(input) => input,
        Err(refusal) => return render::refused(printer, refusal),
    };
    let known = match context.known() {
        Ok(known) => known,
        Err(refusal) => return render::refused(printer, *refusal),
    };
    let mut plugins = plugins(context);
    plugins.environment.day_start = known.day_start().map(OsString::from);
    plugins.known = Some(&known);
    match board.call(target, &input, &plugins) {
        Ok(report) => render::call(printer, &report),
        Err(error) => render::refused(printer, error.refusal()),
    }
}

/// `type add`: the schema on stdin, with a note when the board already
/// held and committed it.
pub(crate) fn type_add(
    command: TypeCommand,
    board: &Board,
    printer: Printer,
    stdin: &mut dyn Read,
) -> Output {
    let TypeCommand::Add { record_type } = command;
    let bytes = match bounded(stdin) {
        Ok(bytes) => bytes,
        Err(error) => return render::refused(printer, unreadable(Code::RepoUnreadable, &error)),
    };
    match board.type_add(&record_type, &bytes) {
        Ok(report) => {
            let held = report.writes.is_empty() && report.commit.is_none();
            let note = format!(
                "note: the board already holds this schema for {record_type}, so nothing is written"
            );
            render::put(printer, &report, held.then_some(note))
        }
        Err(error) => render::refused(printer, error.refusal()),
    }
}

/// stdin, read one byte past the record bound so a parse sees input that is
/// too large.
fn bounded(stdin: &mut dyn Read) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    stdin
        .take(MAX_RECORD_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// The refusal, with `code`, of stdin that cannot be read.
fn unreadable(code: Code, error: &std::io::Error) -> Refusal {
    plain(code, &format!("stdin cannot be read: {error}"))
}
