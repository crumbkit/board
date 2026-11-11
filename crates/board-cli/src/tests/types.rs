//! board-types describes what Board prints: a document written through its
//! types is one the schema `board schemas show` prints admits, what Board
//! prints reads through them and writes back the same JSON, and so does
//! every failure the recoveries check provokes.

use std::collections::{BTreeMap, BTreeSet};

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
            related: vec![Related {
                message: "below".to_owned(),
                severity: Severity::Advice,
                code: None,
                cause: None,
                help: None,
                url: None,
                labels: Vec::new(),
                related: Vec::new(),
            }],
        }],
        recovery: Recovery::Run {
            call: "board ls <PATH>...".to_owned(),
            all: Some("path".to_owned()),
        },
        alternatives: vec![
            Recovery::Terminal {
                call: "board login".to_owned(),
            },
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

/// `every_failure` once with each recovery kind this crate knows, each
/// offering every kind as an alternative.
fn every_recovery() -> Vec<Failure> {
    let failure = every_failure();
    let kinds: Vec<Recovery> = [failure.recovery.clone()]
        .into_iter()
        .chain(failure.alternatives.clone())
        .collect();
    kinds
        .iter()
        .map(|recovery| Failure {
            recovery: recovery.clone(),
            alternatives: kinds.clone(),
            ..failure.clone()
        })
        .collect()
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
        failures: Some(every_recovery()),
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
        failures: Some(every_recovery()),
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

/// Each object `schema` describes, by where it is, with the properties the
/// schema names and the keys `document` holds there across every array
/// element. A branch of a `oneOf` whose `kind` is a constant is the place
/// `/|<kind>`, and holds only the values of that kind. A reference into
/// `$defs` is followed; an anchor reference, which recurses, is not.
fn coverage(
    root: &Value,
    schema: &Value,
    document: Option<&Value>,
    at: &str,
    found: &mut BTreeMap<String, (BTreeSet<String>, BTreeSet<String>)>,
) {
    if let Some(name) = schema["$ref"]
        .as_str()
        .and_then(|reference| reference.strip_prefix("#/$defs/"))
    {
        return coverage(root, &root["$defs"][name], document, at, found);
    }
    if let Some(properties) = schema["properties"].as_object() {
        let place = found.entry(at.to_owned()).or_default();
        place.0.extend(properties.keys().cloned());
        if let Some(object) = document.and_then(Value::as_object) {
            place.1.extend(object.keys().cloned());
        }
        for (name, property) in properties {
            let value = document.and_then(|document| document.get(name));
            coverage(root, property, value, &format!("{at}/{name}"), found);
        }
    }
    if let Some(items) = schema.get("items") {
        let elements = document.and_then(Value::as_array).map(Vec::as_slice);
        match elements {
            Some(elements) if !elements.is_empty() => {
                for element in elements {
                    coverage(root, items, Some(element), &format!("{at}[]"), found);
                }
            }
            _ => coverage(root, items, None, &format!("{at}[]"), found),
        }
    }
    for (n, branch) in schema["oneOf"].as_array().into_iter().flatten().enumerate() {
        match branch["properties"]["kind"]["const"].as_str() {
            Some(kind) => {
                let value = document.filter(|document| document["kind"] == kind);
                coverage(root, branch, value, &format!("{at}|{kind}"), found);
            }
            None => coverage(root, branch, document, &format!("{at}|{n}"), found),
        }
    }
}

#[test]
fn board_types_covers_every_property_and_recovery_kind_the_report_schemas_name() {
    let put = Report {
        schema: PutReport::SCHEMA.to_owned(),
        dry_run: true,
        data: PutReport {
            absent: Vec::new(),
            commit: None,
            removed: Vec::new(),
            writes: Vec::new(),
        },
        failures: Some(every_recovery()),
    };
    let file = |content| File::<Value> {
        location: mounted(),
        blob: None,
        content,
        record: Some(json!({"$type": "lr/task/v1"})),
        code: Some("board::record::invalid".to_owned()),
    };
    let get = Report {
        schema: File::SCHEMA.to_owned(),
        dry_run: true,
        data: vec![
            file(Content::Text("{}".to_owned())),
            file(Content::Base64("/wBh".to_owned())),
        ],
        failures: Some(every_recovery()),
    };
    for (id, document) in [
        (PutReport::SCHEMA, serde_json::to_value(put).unwrap()),
        (File::SCHEMA, serde_json::to_value(get).unwrap()),
    ] {
        let scenario = board();
        let output = board_in(scenario.workdir(), &["schemas", "show", id, "--json"]);
        let schema = json(&output)["data"][0]["schema"].clone();
        let mut found = BTreeMap::new();
        coverage(&schema, &schema, Some(&document), "", &mut found);
        assert!(found.contains_key("/failures[]/recovery|terminal"), "{id}");
        for (at, (named, held)) in found {
            assert_eq!(held, named, "{id} at {at:?}");
        }
    }
    let failure = Printed::<PutReport>::Failure(Box::new(every_failure()));
    let written = serde_json::to_value(failure).unwrap();
    let held: BTreeSet<&str> = written
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    let shown = board_in(
        board().workdir(),
        &["schemas", "show", Failure::SCHEMA, "--json"],
    );
    let schema = json(&shown)["data"][0]["schema"].clone();
    let named: BTreeSet<&str> = schema["properties"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(held, named);
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
