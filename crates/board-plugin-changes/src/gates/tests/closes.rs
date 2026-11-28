//! The build gate's `tasks` check passes an open attached task that the
//! plan's Closes at merge section names, and misses a named path that is
//! not a task of the change's.

use serde_json::json;

use super::{HEAD, PLAN, RUN, TASK, given, run};

/// The fixture plan with a Closes at merge section listing `closes`.
fn closing(closes: &str) -> String {
    format!("{PLAN}\n## Closes at merge\n\n{closes}\n")
}

#[test]
fn an_open_task_the_plan_closes_at_merge_passes_and_the_check_says_it_closes_then() {
    let records = vec![
        ("/t/a.md", ("tasks/task/v3", json!({"status": "open"}))),
        (RUN, run(&json!({}))),
    ];
    let found = given("build")
        .records(records)
        .with(json!({"head": HEAD}))
        .plan(Some(&closing("- `/t/a.md`, whose work this does.")));
    let (status, detail) = found.check("tasks");
    assert_eq!(status, "pass", "{detail}");
    assert_eq!(
        detail,
        "these tasks close when the pull request merges, as the plan's Closes at merge section \
         says: /t/a.md"
    );
}

#[test]
fn an_open_task_the_plan_does_not_name_still_misses() {
    let records = vec![
        ("/t/a.md", ("tasks/task/v3", json!({"status": "open"}))),
        ("/t/b.md", ("tasks/task/v3", json!({"status": "open"}))),
        (RUN, run(&json!({}))),
    ];
    let found = given("build")
        .records(records)
        .with(json!({"head": HEAD}))
        .plan(Some(&closing("/t/a.md")));
    assert_eq!(
        found.check("tasks"),
        (
            "missing",
            "these tasks are not done or dropped: /t/b.md".to_owned()
        )
    );
}

#[test]
fn a_named_path_that_is_not_an_attached_task_misses_with_the_attach_remedy() {
    let records = vec![(RUN, run(&json!({})))];
    let found = given("build")
        .records(records)
        .with(json!({"head": HEAD}))
        .plan(Some(&closing("/t/gone.md")));
    let (status, detail) = found.check("tasks");
    assert_eq!(status, "missing");
    assert!(detail.contains("names /t/gone.md"), "{detail}");
    assert!(
        detail.contains("`board changes attach --path /changes/t --ref <task>`"),
        "{detail}"
    );
    let remedy = found.remedy("tasks");
    assert!(remedy.contains("## Closes at merge"), "{remedy}");
    assert!(
        remedy.contains("`board changes attach --path /changes/t --ref <task>`"),
        "{remedy}"
    );
}

#[test]
fn the_changes_own_task_may_be_named_though_it_is_not_attached() {
    let records = vec![(RUN, run(&json!({})))];
    let found = given("build")
        .records(records)
        .with(json!({"head": HEAD}))
        .plan(Some(&closing(TASK)));
    assert_eq!(found.check("tasks").0, "pass");
}

#[test]
fn a_named_path_only_matches_a_task_as_a_whole_word() {
    let records = vec![
        (
            "/repos/n/tasks/a.md",
            ("tasks/task/v3", json!({"status": "open"})),
        ),
        (RUN, run(&json!({}))),
    ];
    let found = given("build")
        .records(records)
        .with(json!({"head": HEAD}))
        .plan(Some(&closing("/tasks/a.md")));
    let (status, detail) = found.check("tasks");
    assert_eq!(status, "missing");
    assert!(detail.contains("names /tasks/a.md,"), "{detail}");
}

#[test]
fn a_named_task_already_done_is_not_said_to_close_at_merge() {
    let records = vec![
        ("/t/a.md", ("tasks/task/v3", json!({"status": "done"}))),
        ("/t/b.md", ("tasks/task/v3", json!({"status": "open"}))),
        (RUN, run(&json!({}))),
    ];
    let found = given("build")
        .records(records)
        .with(json!({"head": HEAD}))
        .plan(Some(&closing("/t/a.md /t/b.md")));
    let (status, detail) = found.check("tasks");
    assert_eq!(status, "pass", "{detail}");
    assert!(detail.ends_with("says: /t/b.md"), "{detail}");
    let closed = vec![
        ("/t/a.md", ("tasks/task/v3", json!({"status": "done"}))),
        (RUN, run(&json!({}))),
    ];
    let found = given("build")
        .records(closed)
        .with(json!({"head": HEAD}))
        .plan(Some(&closing("/t/a.md")));
    assert_eq!(found.check("tasks"), ("pass", String::new()));
}
