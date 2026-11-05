//! `changes.migrate`, which rewrites change records of the shape the Python
//! plugin wrote into the split shape.

use super::*;

const PULL: &str = "https://github.com/o/r/pull/1";
const ISSUE: &str = "https://github.com/o/r/issues/2";

/// A board holding what the Python plugin wrote: a change with a plan
/// file, attached records and GitHub pages, a prototype, the last sync's
/// day, and a change with a field no shape has. With `github`, the github
/// plugin is enabled and holds the pull request and the issue, so
/// `github.ref` runs no gh.
fn old_board(github: bool) -> Changes {
    let plugins: &[&str] = if github {
        &["changes", "controls", "github"]
    } else {
        &["changes", "controls"]
    };
    let board = Changes::enabling(plugins);
    board.mount_x(&[("README.md", "x\n")]);
    board.write("changes/old/plan.md", PLAN);
    board.write(
        "changes/old.json",
        &change(
            "build",
            &json!({
                "plan": "/changes/old/plan.md",
                "records": [TASK],
                "pull_request": PULL,
                "issue": ISSUE,
            }),
        ),
    );
    board.write(
        "changes/proto.json",
        &change(
            "build",
            &json!({"kind": "prototype", "question": "Does it read better?", "state": "exploring"}),
        ),
    );
    board.write(
        "changes/sync.json",
        &typed("changes/sync/v1", &json!({"synced": days_ago(1)})),
    );
    board.write(
        "changes/odd.json",
        &change("build", &json!({"owner": "someone"})),
    );
    if github {
        for (path, kind, number, url) in [
            ("gh/o/r/pulls/1.json", "github/pull/v1", 1, PULL),
            ("gh/o/r/issues/2.json", "github/issue/v1", 2, ISSUE),
        ] {
            let body = json!({"owner": "o", "repo": "r", "number": number, "url": url});
            board.write(path, &typed(kind, &body));
        }
    }
    board
}

#[test]
fn a_dry_run_says_what_it_would_rewrite_and_what_it_leaves_and_writes_nothing() {
    let board = old_board(true);
    let before = board.text("changes/old.json");
    let output = board.call("migrate", &json!({}));
    assert_eq!(
        output,
        json!({
            "applied": false,
            "changes": ["/changes/old", "/changes/proto"],
            "total": 2,
            "truncated": false,
            "plans": 2,
            "prototypes": 1,
            "refs": ["o/r#1", "o/r#2"],
            "left": [{
                "path": "/changes/odd",
                "why": "it holds fields no shape of changes/change/v1 has: owner",
            }],
            "synced": days_ago(1),
        })
    );
    assert_eq!(board.text("changes/old.json"), before);
    assert!(board.exists("changes/sync.json"));
}

#[test]
fn applying_splits_each_old_change_into_records_board_reads_as_valid_and_a_second_run_finds_none() {
    let board = old_board(true);
    assert_eq!(
        board.code("/changes/old").as_deref(),
        Some("board::record::invalid")
    );
    let output = board.call("migrate", &json!({"apply": true}));
    assert_eq!(
        (&output["applied"], &output["total"]),
        (&json!(true), &json!(2))
    );
    let old = board.read("changes/old.json");
    assert_eq!(
        (&old["pulls"], &old["tickets"]),
        (&json!(["/gh/o/r/pulls/1"]), &json!(["/gh/o/r/issues/2"]))
    );
    let plan = old["plan"].as_str().unwrap();
    assert!(plan.starts_with("/changes/old/plans/"), "{old}");
    assert_eq!(
        board.read(&format!("{}.json", plan.trim_start_matches('/'))),
        json!({
            "$type": "changes/plan/v1",
            "text": "/changes/old/plan.md",
            "uses": [],
            "records": [TASK],
        })
    );
    assert_eq!(
        board.read("changes/proto/prototype.json"),
        json!({
            "$type": "changes/prototype/v1",
            "change": "/changes/proto",
            "question": "Does it read better?",
            "state": "exploring",
        })
    );
    assert!(!board.exists("changes/sync.json"));
    let sync = board.call("open", &json!({}))["sync"].clone();
    assert_eq!(
        (&sync["synced"], &sync["stale"]),
        (&json!(days_ago(1)), &json!(true))
    );
    let degraded: Vec<(String, Option<String>)> = (board.checked("/changes/").into_iter())
        .filter(|(_, code)| code.is_some())
        .collect();
    assert_eq!(
        degraded,
        [(
            "/changes/odd".to_owned(),
            Some("board::record::invalid".to_owned())
        )]
    );
    let again = board.call("migrate", &json!({"apply": true}));
    assert_eq!(
        (&again["total"], &again["synced"]),
        (&json!(0), &Value::Null)
    );
}

#[test]
fn applying_without_the_github_plugin_is_refused_and_writes_nothing_and_with_it_is_admitted() {
    let board = old_board(false);
    let before = board.text("changes/old.json");
    let refused = board.refusal("migrate", &json!({"apply": true}));
    assert_eq!(refused["code"], "changes::board::refused", "{refused}");
    assert_eq!(board.text("changes/old.json"), before);
    let admitted = old_board(true);
    assert_eq!(
        admitted.call("migrate", &json!({"apply": true}))["applied"],
        true
    );
}
