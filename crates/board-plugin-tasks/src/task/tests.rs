//! The model reads what Python's pydantic model read, and counts what it
//! refuses.

use serde_json::json;

use super::*;

fn task(extra: Value) -> Value {
    let mut record = json!({"$type": "tasks/task/v2", "title": "T", "status": "open"});
    let Value::Object(extra) = extra else {
        panic!("{extra} is an object")
    };
    record.as_object_mut().unwrap().extend(extra);
    record
}

#[test]
fn a_task_with_fields_another_tool_writes_is_read_and_one_lacking_its_own_is_refused_counting_each()
{
    let read = model(
        &task(json!({"id": "0f3c9a", "pinned": true, "contexts": ["home"]})),
        0,
    )
    .unwrap();
    assert_eq!(
        (read.title.as_str(), read.contexts),
        ("T", vec!["home".to_owned()])
    );
    assert_eq!(model(&json!({"id": "x"}), 0), Err(3));
    assert_eq!(model(&json!("text"), 0), Err(1));
    let wrong =
        task(json!({"status": "waiting", "due": "2026-13-01", "contexts": ["ok", "Not A Tag"]}));
    assert_eq!(model(&wrong, 0), Err(3));
}

#[test]
fn each_field_the_model_refuses_has_a_twin_it_reads() {
    let pairs = [
        (
            json!({"defer": "2026-06-30T23:59:60Z"}),
            json!({"defer": "2026-06-30T23:59:59Z"}),
        ),
        (json!({"defer": "tomorrow"}), json!({"defer": "soon"})),
        (
            json!({"repeat": "fortnightly"}),
            json!({"repeat": "weekly-mon"}),
        ),
        (json!({"from_hour": 24}), json!({"from_hour": 23})),
        (
            json!({"owner": "lead"}),
            json!({"owner": {"person": "lead"}}),
        ),
        (
            json!({"owner": {}}),
            json!({"owner": {"session": "local_0a1b"}}),
        ),
        (json!({"kind": "epic"}), json!({"kind": "project"})),
        (
            json!({"parent": ["/tasks/a.md"]}),
            json!({"parent": "/tasks/a.md"}),
        ),
        (
            json!({"after": "/tasks/a.md"}),
            json!({"after": ["/tasks/a.md"]}),
        ),
        (json!({"reviewed": "2026-02-30"}), json!({"reviewed": null})),
    ];
    for (refused, admitted) in pairs {
        assert_eq!(model(&task(refused.clone()), 0), Err(1), "{refused}");
        assert!(model(&task(admitted.clone()), 0).is_ok(), "{admitted}");
    }
}

#[test]
fn repeat_is_one_of_the_five_rule_forms() {
    for rule in [
        "daily",
        "weekdays",
        "weekly-mon",
        "monthly-31",
        "monthly-1",
        "annually-02-29",
    ] {
        assert!(repeats(rule), "{rule}");
    }
    for rule in [
        "weekly-monday",
        "monthly-32",
        "monthly-01",
        "annually-13-01",
        "annually-2-29",
        "hourly",
        "daily-1",
    ] {
        assert!(!repeats(rule), "{rule}");
    }
}
