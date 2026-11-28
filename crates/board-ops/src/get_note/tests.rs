//! Board collects every built-in plugin's notes beside a record, and reads
//! every path beside the records asked for in one read.

use std::cell::RefCell;

use board_model::{CapabilityName, Code, LogicalPath, RecordType};
use board_plugin::Manifest;
use board_scenario::Item;
use serde_json::{Value, json};

use super::{Asked, Beside, GetNote, lines};
use crate::read::ReadError;
use crate::tests::{board, open, task};
use crate::{Host, Plugin, PluginError};

fn path(text: &str) -> LogicalPath {
    text.parse().unwrap()
}

/// `/notes/<name>` beside the record at `/<folder>/<name>`.
fn beside_notes(asked: &Asked) -> Option<LogicalPath> {
    let name = asked.path.as_str().rsplit('/').next()?;
    Some(path(&format!("/notes/{name}")))
}

/// Nothing beside a record that is `quiet`, and `/notes/<name>` otherwise.
fn beside_unless_quiet(asked: &Asked) -> Option<LogicalPath> {
    if asked.record.contains_key("quiet") {
        return None;
    }
    beside_notes(asked)
}

/// `<path>: ` and what was found beside it.
#[expect(
    clippy::unnecessary_wraps,
    reason = "GetNote::line returns an Option, so that a note can print nothing"
)]
fn said(asked: &Asked, beside: &Beside<'_>) -> Option<String> {
    let what = match beside {
        Beside::Record { path, record } => format!("{path} holds {}", record["$type"]),
        Beside::Nothing { path } => format!("nothing at {path}"),
        Beside::Failed { path, code } => format!("{path} not read: {code}"),
    };
    Some(format!("{}: {what}", asked.path))
}

static TASK_NOTES: [GetNote; 1] = [GetNote {
    record_type: "lr/task/v1",
    beside: beside_notes,
    line: said,
}];

static PROJECT_NOTES: [GetNote; 1] = [GetNote {
    record_type: "lr/project/v1",
    beside: beside_unless_quiet,
    line: said,
}];

/// A built-in plugin that declares only `notes`.
struct Noting {
    name: &'static str,
    notes: &'static [GetNote],
}

impl Plugin for Noting {
    fn manifest(&self) -> Manifest {
        Manifest::built_in(json!({
            "$type": "board/plugin-manifest/v1",
            "name": self.name,
            "version": "0.1.0",
            "description": "",
            "capabilities": [],
        }))
        .unwrap()
    }

    fn schemas(&self) -> Vec<(RecordType, Value)> {
        Vec::new()
    }

    fn call(&self, _: &dyn Host, _: &CapabilityName, _: Value) -> Result<Value, PluginError> {
        unreachable!("the fixture offers no capability")
    }

    fn get_notes(&self) -> &'static [GetNote] {
        self.notes
    }
}

static PLUGINS: [&dyn Plugin; 2] = [
    &Noting {
        name: "tasking",
        notes: &TASK_NOTES,
    },
    &Noting {
        name: "projecting",
        notes: &PROJECT_NOTES,
    },
];

/// A board with tasks a and b, a project p, and a project at /notes/a.
fn noted_board() -> board_scenario::Scenario {
    let scenario = board();
    scenario
        .write("tasks/a.json", Item::File(&task("")))
        .write("tasks/b.json", Item::File(&task("")))
        .write("notes/a.json", Item::File(r#"{"$type": "lr/project/v1"}"#));
    scenario
}

#[test]
fn board_collects_every_built_in_note_and_names_only_records_of_their_types() {
    let scenario = noted_board();
    scenario.write("notes.md", Item::File("# n\n"));
    let mut board = open(&scenario);
    assert_eq!(board.get_notes().len(), 0);
    board.build_in(&PLUGINS);
    let types: Vec<&str> = (board.get_notes().iter())
        .map(|note| note.record_type)
        .collect();
    assert_eq!(types, ["lr/task/v1", "lr/project/v1"]);
    let asked = |text: &str| {
        let report = board.get(&path(text)).unwrap();
        board.asked(&path(text), &report)
    };
    let task = asked("/tasks/a").unwrap();
    assert_eq!(
        (task.path.as_str(), task.held.as_str()),
        ("/tasks/a", "/tasks/a")
    );
    assert_eq!(task.record["$type"], "lr/task/v1");
    assert!(asked("/projects/p").is_some());
    let note = path("/notes.md");
    let read = (board.get_each(std::slice::from_ref(&note)).unwrap())
        .next()
        .unwrap();
    assert_eq!(board.asked(&note, &read.unwrap()), None, "not a record");
}

#[test]
fn board_reads_beside_each_record_and_gives_each_line_in_order() {
    let scenario = noted_board();
    let mut board = open(&scenario);
    board.build_in(&PLUGINS);
    let asked: Vec<Asked> = ["/tasks/a", "/tasks/b", "/projects/p"]
        .into_iter()
        .map(|text| board.asked(&path(text), &board.get(&path(text)).unwrap()))
        .map(Option::unwrap)
        .collect();
    assert_eq!(
        board.noted(None, &asked),
        [
            "/tasks/a: /notes/a holds \"lr/project/v1\"",
            "/tasks/b: nothing at /notes/b",
            "/projects/p: nothing at /notes/p",
        ]
    );
}

/// An asked record of `record_type` at `text`, `quiet` when it is set.
fn asked(text: &str, record_type: &str, quiet: bool) -> Asked {
    let mut record = json!({"$type": record_type});
    if quiet {
        record["quiet"] = json!(true);
    }
    let Value::Object(record) = record else {
        unreachable!()
    };
    Asked {
        path: path(text),
        display: path(text),
        held: text.to_owned(),
        record,
    }
}

#[test]
fn every_path_beside_the_records_is_read_in_one_read_and_none_when_nothing_is_beside() {
    let notes: Vec<&'static GetNote> = TASK_NOTES.iter().chain(&PROJECT_NOTES).collect();
    let reads: RefCell<Vec<Vec<String>>> = RefCell::new(Vec::new());
    let read = |paths: &[LogicalPath]| {
        reads
            .borrow_mut()
            .push(paths.iter().map(ToString::to_string).collect());
        Err(ReadError::NoHead)
    };
    let asked_for = [
        asked("/tasks/a", "lr/task/v1", false),
        asked("/projects/q", "lr/project/v1", true),
        asked("/projects/p", "lr/project/v1", false),
        asked("/other/o", "lr/other/v1", false),
    ];
    let given = lines(&notes, &asked_for, read);
    assert_eq!(
        reads.borrow().as_slice(),
        [["/notes/a", "/notes/p"]],
        "one read, of the two paths beside"
    );
    assert_eq!(
        given,
        [
            format!("/tasks/a: /notes/a not read: {}", Code::CommitNotFound),
            format!("/projects/p: /notes/p not read: {}", Code::CommitNotFound),
        ],
        "a read that fails at once fails each path beside"
    );
    let quiet = [asked("/projects/q", "lr/project/v1", true)];
    let none = lines(&notes, &quiet, |_| {
        panic!("nothing is beside a quiet project")
    });
    assert_eq!(none, Vec::<String>::new());
}
