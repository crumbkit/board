//! The line editor that the write calls change a task's frontmatter with.
//! These were `test_lines.py`.

use super::*;

const TASK: &str = "---\n$type: tasks/task/v2\ntitle: \"Call the bank\"  # by phone\nstatus: open\nid: abc\n---\n\nBody.\n";

fn set(key: &'static str, value: &str) -> Vec<(&'static str, Option<Value>)> {
    vec![(key, Some(value.into()))]
}

#[test]
fn a_plain_line_is_replaced_a_missing_key_is_added_last_and_every_other_byte_is_kept() {
    let values = [
        ("status", Some("done".into())),
        ("reviewed", Some("2026-10-25".into())),
    ];
    assert_eq!(
        with_fields(TASK, &values, "/tasks/bank.md").unwrap(),
        "---\n$type: tasks/task/v2\ntitle: \"Call the bank\"  # by phone\nstatus: \"done\"\nid: abc\n\
         reviewed: \"2026-10-25\"\n---\n\nBody.\n"
    );
}

#[test]
fn windows_line_endings_are_kept() {
    let text = TASK.replace('\n', "\r\n");
    assert_eq!(
        with_fields(&text, &set("status", "done"), "/t.md").unwrap(),
        text.replace("status: open", "status: \"done\"")
    );
}

#[test]
fn a_key_whose_line_is_not_one_plain_scalar_is_refused_and_a_plain_one_is_replaced() {
    let cases = [
        ("defer: |\n  soon\n", false),
        ("defer:\n  - soon\n", false),
        ("defer: [soon]\n", false),
        ("defer: \"soon\n  later\"\n", false),
        ("defer: soon\ndefer: someday\n", false),
        ("defer: so#on\n", false),
        ("defer: 'so''on\n", false),
        ("defer: soon\n", true),
        ("defer: 2026-11-01  # after the trip\n", true),
        ("defer: \"soon\"\n", true),
        ("defer: 'so''on' # quoted\n", true),
    ];
    for (line, plain) in cases {
        let text = format!("---\ntitle: T\n{line}status: open\n---\n");
        let edited = with_fields(&text, &set("defer", "2026-12-01"), "/tasks/t.md");
        if plain {
            assert!(
                edited.unwrap().contains("defer: \"2026-12-01\"\n"),
                "{line}"
            );
            continue;
        }
        let refused = edited.unwrap_err();
        assert_eq!(refused.code, "tasks::task::not_plain", "{line}");
        assert_eq!(
            Value::Object(refused.fields),
            json!({"path": "/tasks/t.md", "key": "defer"})
        );
        assert!(refused.message.contains("one line"), "{}", refused.message);
    }
}

#[test]
fn a_key_indented_under_another_is_not_the_top_level_key() {
    let text = "---\ntitle: T\nmeta:\n  status: x\nstatus: open\n---\n";
    assert_eq!(
        with_fields(text, &set("status", "done"), "/t.md").unwrap(),
        text.replace("status: open", "status: \"done\"")
    );
}

#[test]
fn a_file_without_a_closed_frontmatter_is_refused_and_one_with_it_is_edited() {
    for text in ["title: T\n", "---\ntitle: T\n"] {
        let refused = with_fields(text, &set("status", "done"), "/t.md").unwrap_err();
        assert_eq!(refused.code, "tasks::task::not_plain");
        let closed = format!("---\n{}---\n", text.trim_start_matches("---\n"));
        let edited = with_fields(&closed, &set("status", "done"), "/t.md").unwrap();
        assert!(edited.ends_with("\"done\"\n---\n"), "{edited}");
    }
}

#[test]
fn a_key_set_to_none_loses_its_line_and_a_missing_one_adds_none() {
    let text = "---\ntitle: T\nparent: \"/tasks/plan.md\"  # moved\nstatus: open\n---\n";
    let cleared = [("parent", None)];
    assert_eq!(
        with_fields(text, &cleared, "/t.md").unwrap(),
        "---\ntitle: T\nstatus: open\n---\n"
    );
    assert_eq!(
        with_fields("---\ntitle: T\n---\n", &cleared, "/t.md").unwrap(),
        "---\ntitle: T\n---\n"
    );
}

/// An owner this editor wrote as a JSON object on one line is replaced,
/// and a YAML flow mapping that is no JSON is still refused.
#[test]
fn a_json_object_line_is_replaced_and_a_flow_mapping_that_is_no_json_is_refused() {
    let owner = |held: &str| format!("---\ntitle: T\nowner: {held}\n---\n");
    let values = [("owner", Some(serde_json::json!({"person": "lead"})))];
    assert_eq!(
        with_fields(&owner(r#"{"session": "local_0a1b"}"#), &values, "/t.md").unwrap(),
        owner(r#"{"person":"lead"}"#)
    );
    let refused = with_fields(&owner("{person: lead}"), &values, "/t.md").unwrap_err();
    assert_eq!(refused.code, "tasks::task::not_plain");
}
