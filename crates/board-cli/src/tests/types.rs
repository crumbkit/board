//! board-types describes what Board prints: a document written through its
//! types is one the schema `board schemas show` prints admits, what Board
//! prints reads through them and writes back the same JSON, and so does
//! every failure the recoveries check provokes.

use std::collections::BTreeMap;

use board_types::{
    Content, Degraded, Excerpt, Failure, FieldValue, File, Label, Location, Pointer, Printed,
    PutReport, Recovery, Related, Removed, Report, Severity, Write,
};

use super::*;

/// The schema `board schemas show <id> --json` prints, compiled as the
/// schema of a document Board prints.
fn shown(id: &str) -> board_validate::Schema {
    let scenario = board();
    let output = board_in(scenario.workdir(), &["schemas", "show", id, "--json"]);
    let printed = json(&output);
    assert_eq!(printed["data"][0]["id"], id);
    board_validate::Schema::compile_document(printed["data"][0]["schema"].clone()).unwrap()
}

/// A failure with every field crumbkit can print.
fn every_failure() -> Failure {
    let label = Label {
        offset: 3,
        length: 2,
        line: Some(1),
        column: Some(4),
        name: Some("a.json".to_owned()),
        text: Some("here".to_owned()),
        excerpt: Some(Excerpt {
            line: 1,
            text: "{\"a\": 1}".to_owned(),
        }),
    };
    let fields = BTreeMap::from([
        ("path".to_owned(), FieldValue::Text("/zz".to_owned())),
        ("found".to_owned(), FieldValue::Null),
        (
            "left_out".to_owned(),
            FieldValue::List(vec![FieldValue::Integer(-1), FieldValue::Unsigned(2)]),
        ),
        (
            "seen".to_owned(),
            FieldValue::Object(BTreeMap::from([(
                "a".to_owned(),
                FieldValue::Boolean(true),
            )])),
        ),
    ]);
    Failure {
        key: Some("/zz".to_owned()),
        index: Some(0),
        code: "board::path::not_found".to_owned(),
        message: "nothing is at /zz".to_owned(),
        cause: Some("a: b".to_owned()),
        fields,
        labels: vec![label.clone()],
        related: vec![Related {
            message: "see".to_owned(),
            severity: Severity::Warning,
            code: Some("c".to_owned()),
            cause: Some("d".to_owned()),
            help: Some("e".to_owned()),
            url: Some("https://example.com/r".to_owned()),
            labels: vec![label],
            related: Vec::new(),
        }],
        recovery: Recovery::Run {
            call: "board ls /".to_owned(),
            all: None,
        },
        alternatives: vec![
            Recovery::Retry,
            Recovery::Hint {
                text: "look".to_owned(),
            },
            Recovery::None {
                reason: "none".to_owned(),
            },
        ],
        url: Some("https://example.com/".to_owned()),
    }
}

/// A location reached through a mount.
fn mounted() -> Location {
    Location {
        display: "/lib/tasks/a".to_owned(),
        path: "tasks/a.json".to_owned(),
        pointers: vec![Pointer {
            follow: Some("main".to_owned()),
            path: "lib.json".to_owned(),
            repository: "/board".to_owned(),
            source: "worktree".to_owned(),
        }],
        repository: "/library".to_owned(),
        source: "fa736015ccf0a4d977752508b6e716d8bbaa028b".to_owned(),
    }
}

/// `document` as JSON, and the same without `field`.
fn and_lacking(document: impl serde::Serialize, field: &str) -> (Value, Value) {
    let full = serde_json::to_value(document).unwrap();
    let mut lacking = full.clone();
    lacking.as_object_mut().unwrap().remove(field);
    (full, lacking)
}

#[test]
fn each_document_board_types_writes_is_admitted_by_its_schema_and_one_lacking_a_field_refused() {
    let put = Report {
        schema: PutReport::SCHEMA.to_owned(),
        dry_run: true,
        data: PutReport {
            absent: vec!["/gone".to_owned()],
            commit: Some("5b1d3c4e5f60718293a4b5c6d7e8f90a1b2c3d4e".to_owned()),
            removed: vec![Removed {
                blob: "0a853c868c31f8e9991aa72662ad9eab6eabbc6b".to_owned(),
                path: "/old".to_owned(),
            }],
            writes: vec![Write {
                blob: "350ab0b8b289baeaf217fc2914e660f59ee25faf".to_owned(),
                degraded: Some(Degraded {
                    code: "board::ref::dangling".to_owned(),
                    failures: 1,
                    pointer: Some("/see".to_owned()),
                    reason: "nothing is at /x".to_owned(),
                }),
                display: "/lib/tasks/a".to_owned(),
                location: mounted(),
                previous: None,
            }],
        },
        failures: Some(vec![every_failure()]),
    };
    let file = |content, record, code: Option<&str>| File {
        location: mounted(),
        blob: Some("350ab0b8b289baeaf217fc2914e660f59ee25faf".to_owned()),
        content,
        record,
        code: code.map(str::to_owned),
    };
    let get = Report {
        schema: File::SCHEMA.to_owned(),
        dry_run: false,
        data: vec![
            file(
                Content::Text("{}".to_owned()),
                Some(json!({"$type": "lr/task/v1"})),
                None,
            ),
            file(
                Content::Base64("/wBh".to_owned()),
                None,
                Some("board::record::not_a_record"),
            ),
        ],
        failures: Some(vec![every_failure()]),
    };
    let failure = Printed::<PutReport>::Failure(Box::new(every_failure()));
    for (id, (full, lacking)) in [
        (PutReport::SCHEMA, and_lacking(put, "schema")),
        (File::SCHEMA, and_lacking(get, "data")),
        (Failure::SCHEMA, and_lacking(failure, "recovery")),
    ] {
        let schema = shown(id);
        if let Err(error) = schema.validate(&full) {
            panic!("{id} refused {full}: {}", error.reason());
        }
        assert_eq!(
            schema.validate(&lacking).unwrap_err().code(),
            Code::RecordInvalid,
            "{id}"
        );
    }
}

#[test]
fn what_put_and_get_print_reads_through_board_types_and_writes_back_the_same_json() {
    let scenario = committing();
    let record = br#"{"title": "a", "$type": "lr/task/v1"}"#;
    let put = board_with(
        scenario.workdir(),
        None,
        &["put", "/tasks/a", "--json"],
        record,
    );
    let printed: Value = json(&put);
    let read: Printed<PutReport> = serde_json::from_value(printed.clone()).unwrap();
    let Printed::Report(report) = &read else {
        panic!("put printed a failure: {printed}")
    };
    assert_eq!(report.data.writes[0].display, "/tasks/a");
    assert_eq!(serde_json::to_value(&read).unwrap(), printed);
    let get = board_in(scenario.workdir(), &["get", "/tasks/a", "/zz", "--json"]);
    let printed: Value = json(&get);
    let read: Printed<Vec<File<Value>>> = serde_json::from_value(printed.clone()).unwrap();
    let Printed::Report(report) = &read else {
        panic!("get printed a failure: {printed}")
    };
    let failures = report.failures.as_deref().unwrap_or_default();
    assert_eq!(failures[0].code, "board::path::not_found");
    assert_eq!(serde_json::to_value(&read).unwrap(), printed);
}

/// Checks that `failure`, a failure document or an entry of a report's
/// `failures`, reads into board-types and writes back the same JSON.
pub(super) fn round_trips(failure: &Value) {
    let read: Failure = serde_json::from_value(failure.clone())
        .unwrap_or_else(|error| panic!("{failure} does not read as a failure: {error}"));
    let mut written = serde_json::to_value(read).unwrap();
    if let Some(schema) = failure.get("schema") {
        written["schema"] = schema.clone();
    }
    assert_eq!(&written, failure);
}
