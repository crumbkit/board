//! `test_sync.py`: `changes.sync`, `changes.drop` and the last sync that
//! `open` and `next` report.
//!
//! A stub gh, first on the plugins' `PATH`, answers
//! `gh api repos/<owner>/<repo>/pulls/<number>` with `pulls/<number>.json`
//! from its own directory, or with GitHub's 404 when that file is missing,
//! and with a 502 when `fail` is there. It records each call in
//! `gh-calls.txt`, so no test reaches GitHub.

use super::*;

const GH: &str = r#"#!/bin/sh
here="$(dirname "$0")"
echo "$@" >> "$here/gh-calls.txt"
if [ -f "$here/fail" ]; then echo "HTTP 502: Bad Gateway" >&2; exit 1; fi
number="${2##*/}"
if [ -f "$here/pulls/$number.json" ]; then cat "$here/pulls/$number.json"; exit 0; fi
echo "gh: Not Found (HTTP 404)" >&2
exit 1
"#;
const MERGE: &str = "0123456789abcdef0123456789abcdef01234567";

/// The URL of pull request `number` of octo/widgets.
fn pull(number: u32) -> String {
    format!("https://github.com/octo/widgets/pull/{number}")
}

/// The fixture board with the stub gh first on its plugins' `PATH`.
fn gh() -> Changes {
    let board = Changes::new();
    board.bin.write("gh", Item::Executable(GH));
    board
}

impl Changes {
    /// Makes the stub gh answer pull request `number` with `fields`.
    fn answers(&self, number: u32, fields: &Value) {
        let answer = record(json!({"number": number}), fields);
        (self.bin).write(&format!("pulls/{number}.json"), Item::Fresh(&answer));
    }

    /// The calls the stub gh recorded, one per line.
    fn gh_calls(&self) -> Vec<String> {
        let calls = std::fs::read_to_string(self.bin.workdir().join("gh-calls.txt")).unwrap();
        calls.lines().map(str::to_owned).collect()
    }
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
        "board call changes.drop --path /changes/bare --reason '<why>'"
    );
    assert_eq!(
        commands["/changes/started"],
        "board call changes.next --path /changes/started"
    );
    assert_eq!(
        commands["/changes/elsewhere"],
        "board call changes.attach --path /changes/elsewhere --pull <record>"
    );
    let tasks: BTreeSet<&str> = (output["changes"].as_array().unwrap().iter())
        .map(|entry| entry["task"].as_str().unwrap())
        .collect();
    assert_eq!(tasks, BTreeSet::from(["/tasks/given.md"]));
    let details = by_path(&output, "detail");
    let merged_detail = details["/changes/merged"].as_str().unwrap();
    assert!(
        !merged_detail.contains("without passing"),
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
        "command": "board call changes.sync",
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
