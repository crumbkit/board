//! `test_sync.py`: `changes.sync`, `changes.drop` and the last sync that
//! `open` and `next` report.
//!
//! The stub gh of [`gh`] answers for GitHub, so no test reaches it.

use super::*;

const MERGE: &str = "0123456789abcdef0123456789abcdef01234567";

/// The URL of pull request `number` of octo/widgets.
fn pull(number: u32) -> String {
    format!("https://github.com/octo/widgets/pull/{number}")
}

/// Changes whose pull requests merged, closed, stayed open and are unknown
/// to GitHub, one whose pull request record Board does not hold, two
/// without one, one still in plan and one in a mounted board.
fn pulls(board: &Changes) {
    board.change_at(
        "changes/merged.json",
        "land",
        &json!({"pull_request": pull(1)}),
    );
    board.answers(
        1,
        &json!({
            "state": "closed",
            "merged": true,
            "merge_commit_sha": MERGE,
            "closed_at": "2026-11-02T10:00:00Z",
        }),
    );
    board.change_at(
        "changes/closed.json",
        "build",
        &json!({"pull_request": pull(2)}),
    );
    board.answers(
        2,
        &json!({
            "state": "closed",
            "merged": false,
            "merge_commit_sha": null,
            "closed_at": "2026-11-03T09:00:00Z",
        }),
    );
    board.change_at(
        "changes/open.json",
        "review",
        &json!({"pull_request": pull(3)}),
    );
    board.answers(3, &json!({"state": "open", "merged": false}));
    board.change_at(
        "changes/gone.json",
        "build",
        &json!({"pull_request": pull(4)}),
    );
    board.change_at(
        "changes/elsewhere.json",
        "build",
        &json!({"pulls": ["/gh/octo/widgets/pulls/9"]}),
    );
    board.change_at("changes/bare.json", "build", &json!({}));
    board.change_at("changes/started.json", "build", &json!({"head": "a3f9c0d"}));
    board.change_at(
        "changes/planning.json",
        "plan",
        &json!({"pull_request": pull(5)}),
    );
    board.write(
        "repos/other/changes/theirs.json",
        &change("build", &json!({"pulls": ["/gh/octo/widgets/pulls/6"]})),
    );
}

/// Each entry's status, the phase it moved to and its code, by path.
fn statuses(output: &Value) -> BTreeMap<String, (Value, Value, Value)> {
    (output["changes"].as_array().unwrap().iter())
        .map(|entry| {
            (
                entry["path"].as_str().unwrap().to_owned(),
                (
                    entry["status"].clone(),
                    entry["to"].clone(),
                    entry["code"].clone(),
                ),
            )
        })
        .collect()
}

/// What a sync of [`pulls`] reports.
fn expected() -> BTreeMap<String, (Value, Value, Value)> {
    [
        (
            "/changes/bare",
            (
                "no_pull_request",
                None,
                Some("changes::sync::no_pull_request"),
            ),
        ),
        ("/changes/closed", ("closed", Some("dropped"), None)),
        (
            "/changes/elsewhere",
            (
                "pull_unreadable",
                None,
                Some("changes::sync::pull_unreadable"),
            ),
        ),
        (
            "/changes/gone",
            ("not_found", None, Some("changes::sync::pull_not_found")),
        ),
        ("/changes/merged", ("merged", Some("done"), None)),
        ("/changes/open", ("open", None, None)),
        (
            "/changes/started",
            (
                "no_pull_request",
                None,
                Some("changes::sync::no_pull_request"),
            ),
        ),
    ]
    .into_iter()
    .map(|(path, (status, to, code))| (path.to_owned(), (json!(status), json!(to), json!(code))))
    .collect()
}

/// Each entry's `field`, by path.
fn by_path(output: &Value, field: &str) -> BTreeMap<String, Value> {
    (output["changes"].as_array().unwrap().iter())
        .map(|entry| {
            (
                entry["path"].as_str().unwrap().to_owned(),
                entry[field].clone(),
            )
        })
        .collect()
}

#[test]
fn sync_ends_merged_and_closed_changes_and_reports_the_rest() {
    let board = gh();
    pulls(&board);
    let output = board.call("sync", &json!({}));
    assert_eq!(statuses(&output), expected());
    assert_eq!(
        (&output["moved"], &output["total"], &output["dry_run"]),
        (&json!(2), &json!(7), &json!(false))
    );
    let merged = board.read("changes/merged.json");
    assert_eq!(
        (&merged["phase"], &merged["merge_commit_url"]),
        (
            &json!("done"),
            &json!(format!("https://github.com/octo/widgets/commit/{MERGE}"))
        )
    );
    let closed = board.read("changes/closed.json");
    assert_eq!(closed["phase"], "dropped");
    assert_eq!(
        closed["dropped_reason"],
        format!(
            "its pull request {} was closed without merging on 2026-11-03",
            pull(2)
        )
    );
    assert_eq!(board.read("changes/open.json")["phase"], "review");
    assert_eq!(board.call("open", &json!({}))["sync"]["synced"], today());
    let commands = by_path(&output, "command");
    assert_eq!(
        commands["/changes/bare"],
        "board changes drop --path /changes/bare --reason '<why>'"
    );
    assert_eq!(
        commands["/changes/started"],
        "board changes next --path /changes/started"
    );
    assert_eq!(
        commands["/changes/elsewhere"],
        "board changes attach --path /changes/elsewhere --pull <record>"
    );
    let tasks: BTreeSet<&str> = (output["changes"].as_array().unwrap().iter())
        .map(|entry| entry["task"].as_str().unwrap())
        .collect();
    assert_eq!(tasks, BTreeSet::from(["/tasks/given.md"]));
    let details = by_path(&output, "detail");
    let merged_detail = details["/changes/merged"].as_str().unwrap();
    assert!(
        merged_detail
            .ends_with("sync moved it from land to done without evaluating the land gates"),
        "{merged_detail}"
    );
    let gone_detail = details["/changes/gone"].as_str().unwrap();
    assert!(
        gone_detail.contains("gh's account cannot see the repository"),
        "{gone_detail}"
    );
    let asked: Vec<String> = [2, 4, 1, 3]
        .iter()
        .map(|number| format!("api repos/octo/widgets/pulls/{number}"))
        .collect();
    assert_eq!(board.gh_calls(), asked);
}

#[test]
fn a_dry_run_reports_the_same_and_writes_nothing() {
    let board = gh();
    pulls(&board);
    let before = board.text("changes/merged.json");
    let output = board.call("sync", &json!({"dry_run": true}));
    assert_eq!(statuses(&output), expected());
    assert_eq!(output["dry_run"], true);
    assert_eq!(board.text("changes/merged.json"), before);
    assert_eq!(
        board.call("open", &json!({}))["sync"]["synced"],
        Value::Null
    );
}

#[test]
fn a_merged_change_with_an_open_task_is_owed_and_moves_once_the_task_is_done() {
    let board = gh();
    board.write("tasks/left.md", &task("open"));
    board.change_at(
        "changes/merged.json",
        "build",
        &json!({"pull_request": pull(1), "records": ["/tasks/left.md"]}),
    );
    board.answers(
        1,
        &json!({"state": "closed", "merged": true, "merge_commit_sha": MERGE}),
    );
    let owed = board.call("sync", &json!({}))["changes"][0].clone();
    assert_eq!(
        (&owed["status"], &owed["to"], &owed["code"]),
        (&json!("owed"), &Value::Null, &json!("changes::sync::owed"))
    );
    let detail = owed["detail"].as_str().unwrap();
    assert!(detail.contains("/tasks/left.md"), "{detail}");
    assert_eq!(board.read("changes/merged.json")["phase"], "build");
    board.write("tasks/left.md", &task("done"));
    assert_eq!(board.call("sync", &json!({}))["changes"][0]["to"], "done");
    assert_eq!(board.read("changes/merged.json")["phase"], "done");
}

/// A board with the tasks plugin and the stub gh, holding a change in
/// land whose merged pull request is 1, with `tasks` attached, each open,
/// and a plan whose Closes at merge section lists `closes`.
fn closing(tasks: &[&str], closes: &str) -> Changes {
    let board = Changes::with_tasks();
    board.bin.write("gh", Item::Executable(GH));
    for each in tasks {
        board.write(each.trim_start_matches('/'), &task("open"));
    }
    board.write("changes/merged/plans/1.md", &landing_plan(closes));
    board.change_at(
        "changes/merged.json",
        "land",
        &json!({
            "pull_request": pull(1),
            "records": tasks,
            "plan": "/changes/merged/plans/1.md",
        }),
    );
    board.answers(
        1,
        &json!({"state": "closed", "merged": true, "merge_commit_sha": MERGE}),
    );
    board
}

/// A plan at land, with its test sections and a Closes at merge section
/// listing `closes`, or none when `closes` is empty.
fn landing_plan(closes: &str) -> String {
    let section = if closes.is_empty() {
        String::new()
    } else {
        format!("## Closes at merge\n\n{closes}\n\n")
    };
    format!(
        "# Plan\n\n{section}## Data\n\nNone.\n\n## What the tests prove\n\nIt closes.\n\n\
         ## What the tests do not prove\n\nRetries.\n"
    )
}

/// Makes the board's `tasks/` directory read-only when `blocked`, so a
/// `tasks.done` there is refused, and writable again when not.
fn block_tasks(board: &Changes, blocked: bool) {
    use std::os::unix::fs::PermissionsExt as _;
    let mode = if blocked { 0o555 } else { 0o755 };
    let directory = board.root().join("tasks");
    std::fs::set_permissions(directory, std::fs::Permissions::from_mode(mode)).unwrap();
}

#[test]
fn advance_refuses_while_an_attached_task_is_open_that_the_plan_no_longer_closes_at_merge() {
    let board = closing(&["/tasks/left.md"], "/tasks/left.md");
    board.write("changes/merged/plans/1.md", &landing_plan(""));
    let refused = board.refusal("advance", &json!({"path": "/changes/merged"}));
    assert_eq!(refused["code"], "changes::land::tasks_open", "{refused}");
    let message = refused["message"].as_str().unwrap();
    assert!(message.contains("/tasks/left.md"), "{message}");
    assert!(message.contains("## Closes at merge"), "{message}");
    assert_eq!(board.read("changes/merged.json")["phase"], "land");
    assert!(board.text("tasks/left.md").contains("status: open"));
    board.write("changes/merged/plans/1.md", &landing_plan("/tasks/left.md"));
    assert_eq!(
        board.call("advance", &json!({"path": "/changes/merged"}))["to"],
        "done"
    );
    assert!(board.text("tasks/left.md").contains(r#"status: "done""#));
}

#[test]
fn a_refused_close_leaves_the_change_in_land_through_advance_and_a_rerun_moves_it() {
    let board = closing(&["/tasks/left.md"], "/tasks/left.md");
    block_tasks(&board, true);
    let refused = board.refusal("advance", &json!({"path": "/changes/merged"}));
    block_tasks(&board, false);
    assert_eq!(
        refused["code"], "changes::land::tasks_not_closed",
        "{refused}"
    );
    assert_eq!(refused["fields"]["task"], "/tasks/left.md", "{refused}");
    assert_eq!(board.read("changes/merged.json")["phase"], "land");
    assert!(board.text("tasks/left.md").contains("status: open"));
    assert_eq!(
        board.call("advance", &json!({"path": "/changes/merged"}))["to"],
        "done"
    );
    assert!(board.text("tasks/left.md").contains(r#"status: "done""#));
}

#[test]
fn a_refused_close_leaves_the_change_in_land_through_sync_and_a_rerun_moves_it() {
    let board = closing(&["/tasks/left.md"], "/tasks/left.md");
    block_tasks(&board, true);
    let entry = board.call("sync", &json!({}))["changes"][0].clone();
    block_tasks(&board, false);
    assert_eq!(entry["code"], "changes::sync::owed", "{entry}");
    let detail = entry["detail"].as_str().unwrap();
    assert!(detail.contains("closing /tasks/left.md"), "{detail}");
    assert!(detail.contains("was refused"), "{detail}");
    assert_eq!(board.read("changes/merged.json")["phase"], "land");
    assert!(board.text("tasks/left.md").contains("status: open"));
    assert_eq!(board.call("sync", &json!({}))["changes"][0]["to"], "done");
    assert!(board.text("tasks/left.md").contains(r#"status: "done""#));
}

/// The development flow's version whose land phase has no pull-request
/// check, so a change on it reaches done with no pull request.
const PULLLESS_FLOW: &str = "/flows/development/7ea3000000000001";

#[test]
fn advance_into_done_with_no_pull_request_closes_no_task_and_refuses_while_a_named_one_is_open() {
    let board = Changes::with_tasks();
    let mut flow: Value = serde_json::from_str(include_str!(
        "../../../../../board-plugin-changes/flows/development.json"
    ))
    .unwrap();
    let land = (flow["phases"].as_array_mut().unwrap().iter_mut())
        .find(|phase| phase["name"] == "land")
        .unwrap();
    land["checks"]
        .as_array_mut()
        .unwrap()
        .retain(|check| check["id"] != "pull-request");
    let version = format!("{}.json", PULLLESS_FLOW.trim_start_matches('/'));
    board.write(&version, &flow.to_string());
    board.write("tasks/left.md", &task("open"));
    board.write("changes/bare/plans/1.md", &landing_plan("/tasks/left.md"));
    board.change_at(
        "changes/bare.json",
        "land",
        &json!({
            "$type": "changes/change/v2",
            "flow": PULLLESS_FLOW,
            "records": ["/tasks/left.md"],
            "plan": "/changes/bare/plans/1.md",
        }),
    );
    let refused = board.refusal("advance", &json!({"path": "/changes/bare"}));
    assert_eq!(refused["code"], "changes::land::no_merge", "{refused}");
    let message = refused["message"].as_str().unwrap();
    assert!(message.contains("/tasks/left.md"), "{message}");
    assert_eq!(board.read("changes/bare.json")["phase"], "land");
    assert!(board.text("tasks/left.md").contains("status: open"));
    board.write("tasks/left.md", &task("done"));
    assert_eq!(
        board.call("advance", &json!({"path": "/changes/bare"}))["to"],
        "done"
    );
}

#[test]
fn sync_closes_each_open_task_the_plan_says_closes_at_merge_and_moves_the_change_to_done() {
    let board = closing(
        &["/tasks/left.md"],
        "- `/tasks/left.md`, whose work this does.",
    );
    let entry = board.call("sync", &json!({}))["changes"][0].clone();
    assert_eq!(
        (&entry["status"], &entry["to"], &entry["code"]),
        (&json!("merged"), &json!("done"), &Value::Null),
        "{entry}"
    );
    let detail = entry["detail"].as_str().unwrap();
    assert!(
        detail.contains("it closed these tasks: /tasks/left.md"),
        "{detail}"
    );
    let left = board.text("tasks/left.md");
    assert!(left.contains(r#"status: "done""#), "{left}");
    assert_eq!(board.read("changes/merged.json")["phase"], "done");
}

#[test]
fn sync_is_owed_only_for_an_open_task_the_plan_does_not_name_and_closes_nothing_until_it_closes() {
    let board = closing(&["/tasks/named.md", "/tasks/left.md"], "/tasks/named.md");
    let owed = board.call("sync", &json!({}))["changes"][0].clone();
    assert_eq!(owed["code"], "changes::sync::owed", "{owed}");
    let detail = owed["detail"].as_str().unwrap();
    assert!(detail.contains("/tasks/left.md"), "{detail}");
    assert!(!detail.contains("/tasks/named.md"), "{detail}");
    assert!(board.text("tasks/named.md").contains("status: open"));
    board.write("tasks/left.md", &task("done"));
    assert_eq!(board.call("sync", &json!({}))["changes"][0]["to"], "done");
    assert!(board.text("tasks/named.md").contains(r#"status: "done""#));
}

#[test]
fn a_dry_run_says_which_tasks_sync_would_close_and_closes_none() {
    let board = closing(&["/tasks/left.md"], "/tasks/left.md");
    let entry = board.call("sync", &json!({"dry_run": true}))["changes"][0].clone();
    assert_eq!(entry["to"], "done", "{entry}");
    let detail = entry["detail"].as_str().unwrap();
    assert!(
        detail.contains("sync would close these tasks: /tasks/left.md"),
        "{detail}"
    );
    assert!(board.text("tasks/left.md").contains("status: open"));
    assert_eq!(board.read("changes/merged.json")["phase"], "land");
}

#[test]
fn a_merged_change_with_an_open_v3_task_is_owed() {
    let board = gh();
    let v3 = task("open").replace("tasks/task/v2", "tasks/task/v3");
    board.write("tasks/left.md", &v3);
    board.change_at(
        "changes/merged.json",
        "build",
        &json!({"pull_request": pull(1), "records": ["/tasks/left.md"]}),
    );
    board.answers(
        1,
        &json!({"state": "closed", "merged": true, "merge_commit_sha": MERGE}),
    );
    let owed = board.call("sync", &json!({}))["changes"][0].clone();
    assert_eq!(owed["status"], "owed", "{owed}");
    board.write("tasks/left.md", &v3.replace("status: open", "status: done"));
    assert_eq!(board.call("sync", &json!({}))["changes"][0]["to"], "done");
}

#[test]
fn a_gh_failure_refuses_the_whole_sync_and_writes_nothing() {
    let board = gh();
    pulls(&board);
    board.bin.write("fail", Item::Fresh(""));
    let refused = board.refusal("sync", &json!({}));
    assert_eq!(refused["code"], "changes::sync::gh_failed");
    let message = refused["message"].as_str().unwrap();
    assert!(message.contains("HTTP 502"), "{message}");
    assert_eq!(board.read("changes/merged.json")["phase"], "land");
    assert_eq!(
        board.call("open", &json!({}))["sync"]["synced"],
        Value::Null
    );
    board.bin.write("fail", Item::Deleted);
    // The merged change and the closed one move.
    assert_eq!(board.call("sync", &json!({}))["moved"], 2);
}

/// What `open` and `next` report of a sync that has not run today, when
/// `stale`, or that ran today.
fn sync_report(synced: &Value, stale: bool) -> Value {
    json!({
        "synced": synced,
        "stale": stale,
        "code": stale.then_some("changes::sync::stale"),
        "command": "board changes sync",
    })
}

#[test]
fn open_and_next_warn_that_the_sync_is_stale_until_it_ran_today() {
    let board = gh();
    board.change_at(
        "changes/t.json",
        "review",
        &json!({"pull_request": pull(3)}),
    );
    board.answers(3, &json!({"state": "open", "merged": false}));
    for output in [
        board.call("open", &json!({})),
        board.call("next", &json!({"path": "/changes/t"})),
    ] {
        assert_eq!(output["sync"], sync_report(&Value::Null, true));
    }
    board.call("sync", &json!({}));
    for output in [
        board.call("open", &json!({})),
        board.call("next", &json!({"path": "/changes/t"})),
    ] {
        assert_eq!(output["sync"], sync_report(&json!(today()), false));
    }
}

#[test]
fn a_change_without_a_pull_request_gets_no_stale_warning_and_open_warns_once_one_has_it() {
    let board = Changes::new();
    board.change_at("changes/t.json", "build", &json!({}));
    for output in [
        board.call("open", &json!({})),
        board.call("next", &json!({"path": "/changes/t"})),
    ] {
        assert_eq!(output["sync"], sync_report(&Value::Null, false));
    }
    board.change_at("changes/u.json", "build", &json!({"pull_request": pull(3)}));
    assert_eq!(
        board.call("open", &json!({}))["sync"],
        sync_report(&Value::Null, true)
    );
    assert_eq!(
        board.call("next", &json!({"path": "/changes/t"}))["sync"],
        sync_report(&Value::Null, false)
    );
    assert_eq!(
        board.call("next", &json!({"path": "/changes/u"}))["sync"],
        sync_report(&Value::Null, true)
    );
}

#[test]
fn a_change_in_plan_or_outside_the_boards_changes_gets_no_stale_warning_for_its_pull_request() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({"pull_request": pull(3)}));
    board.write(
        "repos/other/changes/theirs.json",
        &change("build", &json!({"pulls": ["/gh/octo/widgets/pulls/6"]})),
    );
    for output in [
        board.call("open", &json!({})),
        board.call("next", &json!({"path": "/changes/t"})),
    ] {
        assert_eq!(output["sync"], sync_report(&Value::Null, false));
    }
    // Past its plan, sync reads the change's pull request.
    board.change_at("changes/t.json", "build", &json!({"pull_request": pull(3)}));
    for output in [
        board.call("open", &json!({})),
        board.call("next", &json!({"path": "/changes/t"})),
    ] {
        assert_eq!(output["sync"], sync_report(&Value::Null, true));
    }
}

#[test]
fn drop_ends_an_open_change_with_its_reason_and_refuses_one_already_ended() {
    let board = Changes::new();
    board.change_at("changes/t.json", "build", &json!({}));
    let given = json!({"path": "/changes/t", "reason": "Superseded by another change."});
    assert_eq!(
        board.call("drop", &given),
        json!({
            "path": "/changes/t",
            "from": "build",
            "to": "dropped",
            "task": "/tasks/given.md",
        })
    );
    assert_eq!(
        board.read("changes/t.json")["dropped_reason"],
        "Superseded by another change."
    );
    let refused = board.refusal("drop", &given);
    assert_eq!(
        (&refused["code"], &refused["fields"]["phase"]),
        (&json!("changes::phase::final"), &json!("dropped"))
    );
}

#[test]
fn a_change_that_merged_from_land_ends_and_says_sync_did_not_evaluate_the_land_gates() {
    let board = gh();
    board.change_at(
        "changes/late.json",
        "land",
        &json!({"pull_request": pull(1)}),
    );
    board.answers(
        1,
        &json!({"state": "closed", "merged": true, "merge_commit_sha": MERGE}),
    );
    let found = board.call("sync", &json!({}))["changes"][0].clone();
    assert_eq!(found["to"], "done");
    let detail = found["detail"].as_str().unwrap();
    assert!(
        detail.ends_with("sync moved it from land to done without evaluating the land gates"),
        "{detail}"
    );
}

#[test]
fn a_change_that_merged_from_build_ends_and_says_which_gates_it_skipped() {
    let board = gh();
    board.change_at(
        "changes/early.json",
        "build",
        &json!({"pull_request": pull(1)}),
    );
    board.answers(
        1,
        &json!({"state": "closed", "merged": true, "merge_commit_sha": MERGE}),
    );
    let found = board.call("sync", &json!({}))["changes"][0].clone();
    assert_eq!(found["to"], "done");
    let detail = found["detail"].as_str().unwrap();
    assert!(
        detail.ends_with("it left build without passing the build and review gates"),
        "{detail}"
    );
}

/// Removes the file at `file`, which a test put in the cache.
#[expect(
    clippy::disallowed_methods,
    reason = "the test blocks the cache's directory with a file and then frees it"
)]
fn remove_file(file: &Path) {
    std::fs::remove_file(file).unwrap();
}

#[test]
fn a_sync_whose_day_the_cache_cannot_take_says_why_and_one_it_can_says_nothing() {
    let board = Changes::new();
    board.change_at("changes/t.json", "build", &json!({}));
    // A file where the cache keeps its directory makes every write fail.
    let place = board.cache.path().join("board-changes");
    write_file(&place, "not a directory");
    let output = board.call("sync", &json!({}));
    let why = output["unrecorded"].as_str().unwrap_or_default();
    assert!(
        why.starts_with(&format!(
            "Board's cache did not take {} as the day",
            today()
        )),
        "{output}"
    );
    assert_eq!(
        board.call("open", &json!({}))["sync"]["synced"],
        Value::Null
    );
    remove_file(&place);
    let output = board.call("sync", &json!({}));
    assert_eq!(output.get("unrecorded"), None, "{output}");
    assert_eq!(board.call("open", &json!({}))["sync"]["synced"], today());
}

#[test]
fn a_merged_change_with_a_task_beneath_a_mount_that_cannot_be_read_stays_and_the_others_sync() {
    let board = gh();
    let open = task("open");
    board.mount_repo(&[("tasks/t.md", &open)], "repos/notes", "HEAD");
    board.break_mount("repos/notes", true);
    board.change_at(
        "changes/blocked.json",
        "build",
        &json!({"pull_request": pull(1), "records": ["/repos/notes/tasks/t.md"]}),
    );
    board.change_at(
        "changes/free.json",
        "build",
        &json!({"pull_request": pull(2)}),
    );
    for number in [1, 2] {
        board.answers(
            number,
            &json!({"state": "closed", "merged": true, "merge_commit_sha": MERGE}),
        );
    }
    let synced = board.call("sync", &json!({}));
    let codes = by_path(&synced, "code");
    assert_eq!(
        codes["/changes/blocked"], "changes::sync::unread",
        "{synced}"
    );
    assert_eq!(by_path(&synced, "to")["/changes/blocked"], Value::Null);
    assert_eq!(
        by_path(&synced, "command")["/changes/blocked"],
        Value::Null,
        "{synced}"
    );
    let detail = by_path(&synced, "detail")["/changes/blocked"].clone();
    assert!(
        detail.as_str().unwrap().contains("/repos/notes"),
        "{detail}"
    );
    assert_eq!(board.read("changes/blocked.json")["phase"], "build");
    assert_eq!(board.read("changes/free.json")["phase"], "done");
}
