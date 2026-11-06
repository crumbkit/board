//! The task schema the plugin ships: what `board check` admits and refuses
//! in a Markdown task. These were `test_schema.py`.

use super::*;

/// Each task's code from `board check`, by its path; null when it is
/// valid.
fn checked(tasks: &Tasks) -> BTreeMap<String, Value> {
    let report = json(&tasks.run(&["--json", "check", "--healthy", "--all"], 0, None, ""));
    (report["data"].as_array().unwrap().iter())
        .filter(|entry| entry["logical"].as_str().unwrap().starts_with("/tasks/"))
        .map(|entry| {
            let code = entry.get("code").cloned().unwrap_or(Value::Null);
            (entry["logical"].as_str().unwrap().to_owned(), code)
        })
        .collect()
}

/// The codes `cases` expect, by path: null for valid, else the code.
fn expected(cases: &[(&str, Option<&str>)]) -> BTreeMap<String, Value> {
    (cases.iter())
        .map(|(path, code)| ((*path).to_owned(), json!(code)))
        .collect()
}

const INVALID: Option<&str> = Some("board::record::invalid");

#[test]
fn each_of_the_three_statuses_is_admitted_and_waiting_is_refused() {
    let tasks = Tasks::new();
    for status in ["open", "done", "dropped", "waiting"] {
        tasks.write(
            &format!("tasks/{status}.md"),
            &task(status, status, &json!({}), ""),
        );
    }
    assert_eq!(
        checked(&tasks),
        expected(&[
            ("/tasks/open.md", None),
            ("/tasks/done.md", None),
            ("/tasks/dropped.md", None),
            ("/tasks/waiting.md", INVALID),
        ])
    );
}

#[test]
fn defer_is_a_date_a_date_time_or_a_named_horizon() {
    let tasks = Tasks::new();
    tasks.write(
        "tasks/date.md",
        &open("Date", json!({"defer": "2026-10-26"})),
    );
    let moment = json!({"defer": "2026-10-26T17:00:00+03:00"});
    tasks.write("tasks/time.md", &open("Time", moment));
    tasks.write(
        "tasks/horizon.md",
        &open("Horizon", json!({"defer": "someday"})),
    );
    tasks.write(
        "tasks/unnamed.md",
        &open("Unnamed", json!({"defer": "next-year"})),
    );
    let bare = "---\n$type: tasks/task/v2\ntitle: Bare\nstatus: open\ndefer: 2026-10-26\n---\n";
    tasks.write("tasks/bare.md", bare);
    assert_eq!(
        checked(&tasks),
        expected(&[
            ("/tasks/date.md", None),
            ("/tasks/time.md", None),
            ("/tasks/horizon.md", None),
            ("/tasks/unnamed.md", INVALID),
            ("/tasks/bare.md", None),
        ])
    );
}

#[test]
fn repeat_is_one_of_the_five_rule_forms() {
    let tasks = Tasks::new();
    let admitted = [
        "daily",
        "weekdays",
        "weekly-mon",
        "monthly-31",
        "annually-02-29",
    ];
    let refused = ["weekly-monday", "monthly-32", "annually-13-01", "hourly"];
    for rule in admitted.iter().chain(&refused) {
        tasks.write(
            &format!("tasks/{rule}.md"),
            &open(rule, json!({"repeat": rule})),
        );
    }
    let mut cases: Vec<(String, Option<&str>)> = (admitted.iter())
        .map(|rule| (format!("/tasks/{rule}.md"), None))
        .collect();
    cases.extend(
        refused
            .iter()
            .map(|rule| (format!("/tasks/{rule}.md"), INVALID)),
    );
    let cases: Vec<(&str, Option<&str>)> = cases
        .iter()
        .map(|(path, code)| (path.as_str(), *code))
        .collect();
    assert_eq!(checked(&tasks), expected(&cases));
}

#[test]
fn ephemeral_needs_repeat() {
    let tasks = Tasks::new();
    let repeating = json!({"repeat": "daily", "ephemeral": true});
    tasks.write("tasks/repeating.md", &open("Repeating", repeating));
    tasks.write("tasks/once.md", &open("Once", json!({"ephemeral": true})));
    assert_eq!(
        checked(&tasks),
        expected(&[("/tasks/repeating.md", None), ("/tasks/once.md", INVALID)])
    );
}

#[test]
fn from_hour_is_an_hour_of_the_day_and_contexts_are_lowercase_tags() {
    let tasks = Tasks::new();
    let evening = json!({"from_hour": 23, "contexts": ["phone", "errands"]});
    tasks.write("tasks/evening.md", &open("Evening", evening));
    tasks.write(
        "tasks/midnight.md",
        &open("Midnight", json!({"from_hour": 24})),
    );
    tasks.write(
        "tasks/shouting.md",
        &open("Shouting", json!({"contexts": ["Phone"]})),
    );
    assert_eq!(
        checked(&tasks),
        expected(&[
            ("/tasks/evening.md", None),
            ("/tasks/midnight.md", INVALID),
            ("/tasks/shouting.md", INVALID),
        ])
    );
}

/// The board with a record of another type at `/tasks/thing`.
fn with_a_thing(tasks: &Tasks) {
    tasks.write(
        ".board/types/other/thing/v1.schema.json",
        r#"{"type": "object"}"#,
    );
    tasks.write("tasks/thing.json", r#"{"$type": "other/thing/v1"}"#);
}

#[test]
fn after_references_v2_tasks_by_their_path_with_md() {
    let tasks = Tasks::new();
    tasks.write("tasks/first.md", &open("First", json!({})));
    with_a_thing(&tasks);
    tasks.write(
        "tasks/second.md",
        &open("Second", json!({"after": ["/tasks/first.md"]})),
    );
    tasks.write(
        "tasks/on-thing.md",
        &open("On a thing", json!({"after": ["/tasks/thing"]})),
    );
    tasks.write(
        "tasks/no-suffix.md",
        &open("No suffix", json!({"after": ["/tasks/first"]})),
    );
    let mut codes = checked(&tasks);
    codes.remove("/tasks/thing");
    assert_eq!(
        codes,
        expected(&[
            ("/tasks/first.md", None),
            ("/tasks/second.md", None),
            ("/tasks/on-thing.md", Some("board::ref::wrong_type")),
            ("/tasks/no-suffix.md", Some("board::ref::dangling")),
        ])
    );
}

#[test]
fn a_task_admits_the_id_and_pinned_reflect_writes_and_refuses_a_field_it_does_not_name() {
    let tasks = Tasks::new();
    let reflect = json!({"id": "01m5smm1qj4ye2dw87ne3b9fst", "pinned": true});
    tasks.write("tasks/reflect.md", &open("Reflect", reflect));
    let ordered = json!({"id": "tips-and-tricks", "pinned": 3});
    tasks.write("tasks/ordered.md", &open("Ordered", ordered));
    let shouting = json!({"id": "Tips", "pinned": true});
    tasks.write("tasks/shouting.md", &open("Shouting", shouting));
    let negative = json!({"id": "tips", "pinned": -1});
    tasks.write("tasks/negative.md", &open("Negative", negative));
    let foreign = json!({"programme_id": "ABC-1"});
    tasks.write("tasks/foreign.md", &open("Foreign", foreign));
    let misspelt = json!({"defr": "soon"});
    tasks.write("tasks/misspelt.md", &open("Misspelt", misspelt));
    tasks.write("tasks/spelt.md", &open("Spelt", json!({"defer": "soon"})));
    assert_eq!(
        checked(&tasks),
        expected(&[
            ("/tasks/reflect.md", None),
            ("/tasks/ordered.md", None),
            ("/tasks/shouting.md", INVALID),
            ("/tasks/negative.md", INVALID),
            ("/tasks/foreign.md", INVALID),
            ("/tasks/misspelt.md", INVALID),
            ("/tasks/spelt.md", None),
        ])
    );
}

#[test]
fn put_refuses_a_task_with_a_field_it_does_not_name_naming_the_field_and_writes_its_twin() {
    let tasks = Tasks::new();
    let put = |path: &str, text: &str| {
        let ran = tasks.run(&["--json", "put", path, "--file"], 0, None, text);
        (ran.code, json(&ran))
    };
    let foreign = open("Foreign", json!({"programme_id": "ABC-1"}));
    let (code, document) = put("/tasks/foreign.md", &foreign);
    assert_eq!(code, 1, "{document}");
    assert_eq!(document["code"], "board::record::invalid", "{document}");
    let message = document["message"].as_str().unwrap();
    assert!(message.contains("'programme_id'"), "{message}");
    let (code, document) = put("/tasks/foreign.md", &open("Foreign", json!({})));
    assert_eq!(code, 0, "{document}");
}

#[test]
fn owner_is_an_actor_with_at_least_one_part() {
    let tasks = Tasks::new();
    tasks.write(
        "tasks/owned.md",
        &open("Owned", json!({"owner": {"person": "session-7f3a"}})),
    );
    tasks.write("tasks/empty.md", &open("Empty", json!({"owner": {}})));
    tasks.write("tasks/number.md", &open("Number", json!({"owner": 7})));
    assert_eq!(
        checked(&tasks),
        expected(&[
            ("/tasks/owned.md", None),
            ("/tasks/empty.md", INVALID),
            ("/tasks/number.md", INVALID),
        ])
    );
}

#[test]
fn parent_references_a_v2_task_by_its_path_with_md() {
    let tasks = Tasks::new();
    tasks.write("tasks/plan.md", &open("Plan", json!({})));
    with_a_thing(&tasks);
    tasks.write(
        "tasks/piece.md",
        &open("Piece", json!({"parent": "/tasks/plan.md"})),
    );
    let under = json!({"parent": "/tasks/thing"});
    tasks.write("tasks/under-thing.md", &open("Under a thing", under));
    tasks.write(
        "tasks/no-suffix.md",
        &open("No suffix", json!({"parent": "/tasks/plan"})),
    );
    tasks.write(
        "tasks/two.md",
        &open("Two parents", json!({"parent": ["/tasks/plan.md"]})),
    );
    let mut codes = checked(&tasks);
    codes.remove("/tasks/thing");
    assert_eq!(
        codes,
        expected(&[
            ("/tasks/plan.md", None),
            ("/tasks/piece.md", None),
            ("/tasks/under-thing.md", Some("board::ref::wrong_type")),
            ("/tasks/no-suffix.md", Some("board::ref::dangling")),
            ("/tasks/two.md", INVALID),
        ])
    );
}

#[test]
fn kind_is_task_or_project_and_brief_references_any_file() {
    let tasks = Tasks::new();
    tasks.write("projects/flows/README.md", "# Flows\n");
    let project = json!({"kind": "project", "brief": "/projects/flows/README.md"});
    tasks.write("tasks/project.md", &open("Project", project));
    tasks.write("tasks/task.md", &open("Task", json!({"kind": "task"})));
    tasks.write("tasks/epic.md", &open("Epic", json!({"kind": "epic"})));
    let lost = json!({"kind": "project", "brief": "/projects/gone/README.md"});
    tasks.write("tasks/lost.md", &open("Lost", lost));
    assert_eq!(
        checked(&tasks),
        expected(&[
            ("/tasks/project.md", None),
            ("/tasks/task.md", None),
            ("/tasks/epic.md", INVALID),
            ("/tasks/lost.md", Some("board::ref::dangling")),
        ])
    );
}
