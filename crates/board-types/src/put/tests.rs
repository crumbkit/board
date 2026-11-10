//! A put report reads as Board prints it, and a batch line reads each of
//! its forms and refuses what Board refuses.

use serde_json::{Value, json};

use super::*;

/// The data of a put report that wrote through a mount and removed a file.
fn written() -> Value {
    json!({
        "absent": ["/gone"],
        "commit": "5b1d3c4e5f60718293a4b5c6d7e8f90a1b2c3d4e",
        "removed": [{"blob": "0a853c868c31f8e9991aa72662ad9eab6eabbc6b", "path": "/old"}],
        "writes": [{
            "blob": "350ab0b8b289baeaf217fc2914e660f59ee25faf",
            "degraded": {"code": "board::ref::dangling", "failures": 1, "pointer": "/see", "reason": "nothing is at /x"},
            "display": "/lib/tasks/a",
            "location": {
                "display": "/lib/tasks/a",
                "path": "tasks/a.json",
                "pointers": [{"follow": "main", "path": "lib.json", "repository": "/board", "source": "worktree"}],
                "repository": "/library",
                "source": "fa736015ccf0a4d977752508b6e716d8bbaa028b"
            },
            "previous": null
        }]
    })
}

#[test]
fn a_put_report_reads_and_writes_back_the_same_json() {
    let new = json!({"commit": null, "writes": [{"blob": "b", "degraded": null, "display": "/a", "location": {"display": "/a", "path": "a.json", "pointers": [], "repository": "/r", "source": "worktree"}, "previous": null}]});
    for value in [written(), new] {
        let report: PutReport = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(&report).unwrap(), value);
    }
}

/// Each form of a batch line, as `board put --batch` reads it.
fn forms() -> Vec<(Value, BatchLine<Value>)> {
    let record = json!({"$type": "lr/task/v1"});
    vec![
        (
            json!({"path": "/tasks/a", "expect": "none", "record": record}),
            BatchLine::Record {
                path: "/tasks/a".to_owned(),
                expect: Some("none".to_owned()),
                record: record.clone(),
            },
        ),
        (
            json!({"path": "/notes/a.md", "text": "# a\n"}),
            BatchLine::Text {
                path: "/notes/a.md".to_owned(),
                expect: None,
                text: "# a\n".to_owned(),
            },
        ),
        (
            json!({"path": "/tasks/b", "remove": true, "ignore_unmatch": true}),
            BatchLine::Remove {
                path: "/tasks/b".to_owned(),
                expect: None,
                ignore_unmatch: true,
            },
        ),
        (
            json!({"new": "/suites/a/runs/", "record": record}),
            BatchLine::New {
                new: "/suites/a/runs/".to_owned(),
                record,
            },
        ),
    ]
}

#[test]
fn each_batch_line_form_reads_and_writes_back_the_same_json() {
    for (value, line) in forms() {
        assert_eq!(
            serde_json::from_value::<BatchLine<Value>>(value.clone()).unwrap(),
            line
        );
        assert_eq!(serde_json::to_value(&line).unwrap(), value);
    }
}

#[test]
fn a_batch_line_refuses_a_field_it_does_not_know_and_admits_its_twin() {
    let refused = json!({"path": "/tasks/a", "record": {}, "later": 1});
    let error = serde_json::from_value::<BatchLine<Value>>(refused).unwrap_err();
    assert!(
        error.to_string().starts_with("unknown field `later`"),
        "{error}"
    );
    let twin = json!({"path": "/tasks/a", "record": {}});
    assert!(serde_json::from_value::<BatchLine<Value>>(twin).is_ok());
}

#[test]
fn a_batch_line_refuses_fields_that_do_not_fit_together_with_boards_reason() {
    for (refused, reason, admitted) in [
        (
            json!({"path": "/a", "new": "/b/", "record": {}}),
            "it holds both path and new",
            json!({"path": "/a", "record": {}}),
        ),
        (
            json!({"record": {}}),
            "it holds no path or new",
            json!({"new": "/b/", "record": {}}),
        ),
        (
            json!({"new": "/b/", "expect": "none", "record": {}}),
            "a new line holds a record and no expect, since its name is new",
            json!({"new": "/b/", "record": {}}),
        ),
        (
            json!({"new": "/b/", "text": "t"}),
            "a new line holds a record and no expect, since its name is new",
            json!({"path": "/b", "text": "t"}),
        ),
        (
            json!({"path": "/a", "text": "t", "ignore_unmatch": true}),
            "it ignores an unmatched path and does not remove",
            json!({"path": "/a", "remove": true, "ignore_unmatch": true}),
        ),
        (
            json!({"new": "/b/", "record": {}, "ignore_unmatch": true}),
            "it ignores an unmatched path and does not remove",
            json!({"new": "/b/", "record": {}}),
        ),
        (
            json!({"path": "/a", "record": {}, "text": "t"}),
            "it holds both record and text",
            json!({"path": "/a", "text": "t"}),
        ),
        (
            json!({"new": "/b/", "record": {}, "text": "t"}),
            "it holds both record and text",
            json!({"new": "/b/", "record": {}}),
        ),
        (
            json!({"path": "/a", "text": "t", "remove": true}),
            "it removes and also holds a record or text",
            json!({"path": "/a", "remove": true}),
        ),
        (
            json!({"new": "/b/", "record": {}, "remove": true}),
            "it removes and also holds a record or text",
            json!({"new": "/b/", "record": {}, "remove": false}),
        ),
        (
            json!({"path": "/a", "expect": "none"}),
            "it holds no record or text and does not remove",
            json!({"path": "/a", "expect": "none", "remove": true}),
        ),
    ] {
        let error = serde_json::from_value::<BatchLine<Value>>(refused.clone()).unwrap_err();
        assert_eq!(error.to_string(), reason, "{refused}");
        assert!(
            serde_json::from_value::<BatchLine<Value>>(admitted.clone()).is_ok(),
            "{admitted}"
        );
    }
}
