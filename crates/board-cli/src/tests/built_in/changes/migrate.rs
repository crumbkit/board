//! `changes.migrate`, which rewrites each `changes/change/v1` as a
//! `changes/change/v2`, and change records of the shape the Python plugin
//! wrote into the split shape first.

use super::*;

const PULL: &str = "https://github.com/o/r/pull/1";
const ISSUE: &str = "https://github.com/o/r/issues/2";

/// A board holding what the Python plugin wrote: a change with a plan
/// file, attached records and GitHub pages, a prototype, the last sync's
/// day, an understanding check, and a change with a field no shape has. With `github`, the github
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
        "changes/old/understanding/data.json",
        &typed(
            "changes/understanding/v1",
            &json!({
                "change": "/changes/old",
                "subject": "data",
                "playback": "One record per call.",
                "questions": [{"round": 1, "question": "Q?", "answer": "A.", "matches": true}],
            }),
        ),
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
            "left": [
                {
                    "path": "/changes/odd",
                    "why": "it holds fields no shape of changes/change/v1 has: owner",
                },
                {
                    "path": "/changes/old/understanding/data",
                    "why": "it is an understanding check of the old shape; give each question the \
                            design's answer in `design`, and keep only the fields \
                            changes/understanding/v1 has",
                },
            ],
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
    assert_eq!(
        (&old["$type"], &old["surfaced"]),
        (&json!("changes/change/v2"), &json!([]))
    );
    let version = old["flow"].as_str().unwrap();
    assert!(version.starts_with("/flows/development/"), "{old}");
    assert_eq!(
        board.read(&format!("{}.json", version.trim_start_matches('/')))["name"],
        "development"
    );
    assert!(!board.exists("changes/old/flow.json"));
    let proto = board.read("changes/proto.json");
    assert_eq!(
        (&proto["$type"], &proto["flow"]),
        (&json!("changes/change/v2"), &Value::Null)
    );
    assert!(!board.exists("changes/proto/flow.json"));
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
            "question": "Does it read better?",
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
    let invalid = Some("board::record::invalid".to_owned());
    assert_eq!(
        degraded,
        [
            ("/changes/odd".to_owned(), invalid.clone()),
            ("/changes/old/understanding/data".to_owned(), invalid),
        ]
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

/// A stub gh that answers every call with GitHub's 404.
const GH_NOT_FOUND: &str = "#!/bin/sh\necho \"gh: Not Found (HTTP 404)\" >&2\nexit 1\n";

#[test]
fn a_page_github_ref_cannot_record_leaves_its_change_and_the_rest_migrate() {
    let board = old_board(true);
    board.bin.write("gh", Item::Executable(GH_NOT_FOUND));
    // Without its record, github.ref asks gh for the issue, which GitHub
    // does not have.
    board.delete("gh/o/r/issues/2.json");
    let before = board.text("changes/old.json");
    let output = board.call("migrate", &json!({"apply": true}));
    assert_eq!(
        (&output["changes"], &output["total"]),
        (&json!(["/changes/proto"]), &json!(1))
    );
    let left = output["left"].as_array().unwrap();
    let old = left
        .iter()
        .find(|each| each["path"] == "/changes/old")
        .unwrap();
    assert_eq!(old["page"], ISSUE, "{old}");
    // The pull request's record was written before the issue was refused.
    assert_eq!(old["recorded"], json!(["/gh/o/r/pulls/1"]), "{old}");
    let why = old["why"].as_str().unwrap();
    assert!(
        why.starts_with(&format!(
            "github.ref could not record {ISSUE}: github::ref::not_found: "
        )) && why.contains("GitHub has no such page"),
        "{why}"
    );
    assert_eq!(board.text("changes/old.json"), before);
    assert_eq!(board.code("/changes/proto"), None);
}

#[test]
fn a_dry_run_leaves_a_change_whose_rewrite_fails_its_schema_and_lists_one_that_passes() {
    let board = Changes::new();
    board.mount_x(&[("README.md", "x\n")]);
    board.write(
        "changes/blank.json",
        &change("build", &json!({"route": "sometimes", "records": [TASK]})),
    );
    board.write(
        "changes/fine.json",
        &change("build", &json!({"records": [TASK]})),
    );
    let output = board.call("migrate", &json!({}));
    assert_eq!(output["changes"], json!(["/changes/fine"]));
    let left = &output["left"][0];
    assert_eq!(left["path"], "/changes/blank");
    let why = left["why"].as_str().unwrap();
    assert!(
        why.starts_with("its rewritten record at /changes/blank fails its schema at"),
        "{why}"
    );
}

/// A stub gh that fails as gh does when it is signed out.
const GH_SIGNED_OUT: &str =
    "#!/bin/sh\necho \"To get started with GitHub CLI, please run:  gh auth login\" >&2\nexit 4\n";

#[test]
fn a_gh_that_fails_stops_the_migration_and_says_to_sign_in_and_a_working_gh_migrates() {
    let board = old_board(true);
    board.bin.write("gh", Item::Executable(GH_SIGNED_OUT));
    board.delete("gh/o/r/issues/2.json");
    let before = board.text("changes/old.json");
    let refused = board.refusal("migrate", &json!({"apply": true}));
    assert_eq!(
        (&refused["code"], &refused["fields"]["page"]),
        (&json!("changes::migrate::gh_failed"), &json!(ISSUE)),
        "{refused}"
    );
    let message = refused["message"].as_str().unwrap();
    assert!(message.contains("gh auth login"), "{message}");
    assert_eq!(board.text("changes/old.json"), before);
    // With the issue's record back, gh is not asked and the change migrates.
    let admitted = old_board(true);
    admitted.bin.write("gh", Item::Executable(GH_SIGNED_OUT));
    assert_eq!(
        admitted.call("migrate", &json!({"apply": true}))["total"],
        2
    );
}

#[test]
fn a_prototype_record_already_there_that_agrees_is_kept_and_one_that_differs_or_does_not_read_is_left()
 {
    for (held, left) in [
        (json!({"question": "Does it read better?"}), None),
        (
            json!({"question": "Is it already split?"}),
            Some(
                "the prototype record already at /changes/proto/prototype holds another question than the change",
            ),
        ),
        (
            json!({"question": "Does it read better?", "unknown": true}),
            Some(
                "a prototype record is already at /changes/proto/prototype, and Board does not read it",
            ),
        ),
    ] {
        let board = old_board(true);
        let text = typed("changes/prototype/v1", &held);
        board.write("changes/proto/prototype.json", &text);
        let output = board.call("migrate", &json!({"apply": true}));
        assert_eq!(output["prototypes"], 0, "{held}");
        assert_eq!(board.text("changes/proto/prototype.json"), text);
        let entry = (output["left"].as_array().unwrap().iter())
            .find(|each| each["path"] == "/changes/proto");
        match left {
            None => {
                assert!(entry.is_none(), "{output}");
                assert_eq!(board.code("/changes/proto"), None);
            }
            Some(start) => {
                let why = entry.unwrap()["why"].as_str().unwrap();
                assert!(why.starts_with(start), "{why}");
                assert_eq!(
                    board.code("/changes/proto").as_deref(),
                    Some("board::record::invalid")
                );
            }
        }
    }
}

#[test]
fn applying_rewrites_a_v1_change_in_each_phase_as_a_v2_on_the_development_flow_and_a_second_run_finds_none()
 {
    let board = Changes::new();
    board.mount_x(&[("README.md", "x\n")]);
    let phases = [
        "plan", "build", "review", "land", "done", "dropped", "paused",
    ];
    for phase in phases {
        board.change_at(
            &format!("changes/{phase}.json"),
            phase,
            &json!({"route": "owner"}),
        );
    }
    board.change_at(
        "changes/proto.json",
        "build",
        &json!({"kind": "prototype", "question": "Does it read better?"}),
    );
    let output = board.call("migrate", &json!({"apply": true}));
    assert_eq!(output["total"], json!(phases.len() + 1));
    for phase in phases {
        let change = board.read(&format!("changes/{phase}.json"));
        assert_eq!(
            (&change["$type"], &change["phase"], &change["route"]),
            (&json!("changes/change/v2"), &json!(phase), &json!("owner")),
            "{phase}"
        );
        let version = change["flow"].as_str().unwrap();
        assert!(version.starts_with("/flows/development/"), "{phase}");
        assert_eq!(change["surfaced"], json!([]));
        let stored = board.read(&format!("{}.json", version.trim_start_matches('/')));
        assert_eq!(stored["name"], "development", "{phase}");
    }
    let proto = board.read("changes/proto.json");
    assert_eq!(
        (&proto["$type"], &proto["phase"], &proto["flow"]),
        (&json!("changes/change/v2"), &json!("build"), &Value::Null)
    );
    assert!(!board.exists("changes/proto/flow.json"));
    let degraded: Vec<(String, Option<String>)> = (board.checked("/changes/").into_iter())
        .filter(|(_, code)| code.is_some())
        .collect();
    assert_eq!(degraded, []);
    let next = board.call("next", &json!({"path": "/changes/plan"}));
    assert_eq!(next["flow"]["name"], "development");
    let paused = board.refusal("advance", &json!({"path": "/changes/paused"}));
    assert_eq!(paused["code"], "changes::phase::paused");
    let again = board.call("migrate", &json!({"apply": true}));
    assert_eq!(again["total"], json!(0));
}

#[test]
fn a_v2_change_is_left_as_it_is_and_a_v1_beside_it_is_rewritten() {
    let board = Changes::new();
    let mount = board.mount_x(&[("README.md", "x\n")]);
    let started = board.call(
        "start",
        &json!({"mount": mount, "task": TASK, "title": "New"}),
    );
    let path = started["path"]
        .as_str()
        .unwrap()
        .trim_start_matches('/')
        .to_owned();
    let before = board.text(&format!("{path}.json"));
    board.change_at("changes/t.json", "build", &json!({}));
    let output = board.call("migrate", &json!({"apply": true}));
    assert_eq!(output["changes"], json!(["/changes/t"]));
    assert_eq!(board.text(&format!("{path}.json")), before);
}

/// A v1 build change whose mount's contributing docs name `/suites/checks`,
/// with a passing run of that suite on its head attached, as a v1 change
/// passed `suite-run` before flows.
fn v1_build_change_with_a_passing_run(board: &Changes, docs: &str) {
    board.mount_x(&[("CONTRIBUTING.md", docs)]);
    board.write("suites/checks.json", &typed("runs/suite/v1", &json!({})));
    let run = json!({
        "suite": "/suites/checks",
        "scope": "full",
        "dirty": false,
        "versions": {"x": HEAD},
        "results": [],
    });
    board.write("suites/checks/runs/1.json", &typed("runs/run/v1", &run));
    board.change_at(
        "changes/t.json",
        "build",
        &json!({"head": HEAD, "plan": "/changes/t/plan.md", "records": ["/suites/checks/runs/1"]}),
    );
    board.write("changes/t/plan.md", PLAN);
}

#[test]
fn a_migrated_build_change_keeps_the_suite_its_docs_name_and_its_passing_run_passes_suite_run() {
    let board = Changes::new();
    v1_build_change_with_a_passing_run(&board, "Checks: `/suites/checks`.\n");
    board.call("migrate", &json!({"apply": true}));
    let change = board.read("changes/t.json");
    assert_eq!(
        (&change["$type"], &change["suite"]),
        (&json!("changes/change/v2"), &json!("/suites/checks"))
    );
    let moved = board.call("advance", &json!({"path": "/changes/t"}));
    assert_eq!(moved["to"], "review");
}

#[test]
fn a_migrated_build_change_whose_docs_name_no_suite_has_none_and_misses_suite_run() {
    let board = Changes::new();
    v1_build_change_with_a_passing_run(&board, "Run the checks.\n");
    board.call("migrate", &json!({"apply": true}));
    let change = board.read("changes/t.json");
    assert_eq!(change["suite"], Value::Null);
    let refused = board.refusal("advance", &json!({"path": "/changes/t"}));
    assert_eq!(refused["code"], "changes::gate::unmet");
    assert_eq!(refused["fields"]["missing"], json!(["suite-run"]));
}

#[test]
fn a_migrated_change_in_each_phase_faces_the_blocking_checks_of_the_development_flow() {
    let board = Changes::new();
    board.mount_x(&[("README.md", "x\n")]);
    let phases = ["plan", "build", "review", "land"];
    for phase in phases {
        board.change_at(
            &format!("changes/{phase}.json"),
            phase,
            &json!({"route": "owner", "plan": format!("/changes/{phase}/plan.md")}),
        );
        board.write(&format!("changes/{phase}/plan.md"), PLAN);
    }
    board.call("migrate", &json!({"apply": true}));
    let blocking: Vec<(&str, Vec<String>)> = (phases.iter())
        .map(|phase| {
            let next = board.call("next", &json!({"path": format!("/changes/{phase}")}));
            let ids = (next["gate"].as_array().unwrap().iter())
                .filter(|check| check["blocks"] == json!(true))
                .map(|check| check["check"].as_str().unwrap().to_owned())
                .collect();
            (*phase, ids)
        })
        .collect();
    let expected: Vec<(&str, Vec<String>)> = [
        (
            "plan",
            vec![
                "plan-sections",
                "considered",
                "task",
                "route",
                "owner-approval",
                "lifecycle-declared",
            ],
        ),
        ("build", vec!["head", "suite-run", "tasks"]),
        (
            "review",
            vec![
                "review",
                "paths",
                "ui-paths",
                "data-paths",
                "lifecycle-paths",
                "lifecycle-none-paths",
                "obligations-settled",
            ],
        ),
        (
            "land",
            vec![
                "test-sections",
                "pull-request",
                "lifecycle-paths-at-land",
                "lifecycle-none-paths-at-land",
            ],
        ),
    ]
    .into_iter()
    .map(|(phase, ids)| (phase, ids.into_iter().map(str::to_owned).collect()))
    .collect();
    assert_eq!(blocking, expected);
}

const DEVELOPMENT_FLOW: &str =
    include_str!("../../../../../board-plugin-changes/flows/development.json");
const SMALL_FLOW: &str = include_str!("../../../../../board-plugin-changes/flows/small.json");

/// A board that commits its puts, holding changes written before flow
/// versions: three with the same copy of the development flow, one with a
/// copy of the small flow and one whose copy Board does not read. A run of
/// the first names its flow by name, as runs did before versions; a run of
/// the second names its change's copy, as a run of a change not yet
/// migrated does; and a run of the last names that change's unreadable
/// copy. Everything is committed.
fn copies_board() -> Changes {
    let board = Changes::new();
    board.with_old_run_schema();
    board.mount_x(&[("README.md", "x\n")]);
    let mut config = board.read(".board/board.json");
    config["commit_on_put"] = json!(true);
    board.write(".board/board.json", &config.to_string());
    let copies = [
        ("a", DEVELOPMENT_FLOW.to_owned()),
        ("b", DEVELOPMENT_FLOW.to_owned()),
        ("c", DEVELOPMENT_FLOW.to_owned()),
        ("d", SMALL_FLOW.to_owned()),
        ("e", typed("changes/flow/v1", &json!({"name": "broken"}))),
    ];
    for (name, copy) in copies {
        let phase = if name == "d" { "build" } else { "plan" };
        let change = json!({
            "$type": "changes/change/v2",
            "title": format!("Change {name}"),
            "mount": "/repos/x",
            "flow": format!("/changes/{name}/flow"),
            "phase": phase,
            "surfaced": [],
            "task": TASK,
            "touched": today(),
        });
        board.write(&format!("changes/{name}.json"), &change.to_string());
        board.write(&format!("changes/{name}/flow.json"), &copy);
    }
    for (name, flow) in [
        ("a", "development"),
        ("b", "/changes/b/flow"),
        ("e", "/changes/e/flow"),
    ] {
        board.write(
            &format!("changes/{name}/stops/1.json"),
            &typed(
                "changes/stop/v1",
                &json!({"reason": "reached", "phase": "plan", "missing": [], "summary": "Stopped."}),
            ),
        );
        let run = json!({
            "$type": "changes/run/v1",
            "flow": flow,
            "stop": {"through": "plan"},
            "started": "2026-11-01T10:00:00Z",
            "ended": "2026-11-01T10:05:00Z",
            "sandbox": false,
            "phase": "plan",
            "reason": "reached",
            "stopped": format!("/changes/{name}/stops/1"),
        });
        board.write(&format!("changes/{name}/runs/1.json"), &run.to_string());
    }
    let root = board.root().to_path_buf();
    git(&root, &["config", "user.name", "t"]);
    git(&root, &["config", "user.email", "t@t"]);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "fixture"]);
    board
}

#[test]
fn migrating_flows_collapses_identical_copies_into_one_version_in_one_bulk_commit() {
    let board = copies_board();
    let before = board.board.head().unwrap().id;
    let dry = board.call("migrate", &json!({"flows": true}));
    let flows = &dry["flows"];
    assert_eq!(
        (&flows["applied"], &flows["total"], &flows["copies"]),
        (&json!(false), &json!(4), &json!(4))
    );
    assert_eq!(flows["versions"].as_array().unwrap().len(), 2);
    assert_eq!(
        flows["unreadable"],
        json!([{"path": "/changes/e/flow", "change": "/changes/e", "code": "changes::flow::unreadable"}])
    );
    assert_eq!(
        flows["runs"],
        json!(["/changes/a/runs/1", "/changes/b/runs/1"])
    );
    // The dry run writes nothing.
    assert_eq!(board.board.head().unwrap().id, before);
    assert!(board.exists("changes/a/flow.json"));
    assert!(!board.exists("flows"));
    let applied = board.call("migrate", &json!({"flows": true, "apply": true}));
    let flows = &applied["flows"];
    assert_eq!(
        (&flows["applied"], &flows["changes"], &flows["copies"]),
        (
            &json!(true),
            &json!(["/changes/a", "/changes/b", "/changes/c", "/changes/d"]),
            &json!(4)
        )
    );
    let head = board.board.head().unwrap();
    assert_eq!(head.parents, [before]);
    assert!(
        head.message.ends_with("\n\nBoard-Bulk: migrate flows"),
        "{}",
        head.message
    );
    let followed: Vec<Value> = ["a", "b", "c", "d"]
        .iter()
        .map(|name| board.read(&format!("changes/{name}.json"))["flow"].clone())
        .collect();
    assert_eq!((&followed[0], &followed[0]), (&followed[1], &followed[2]));
    assert_ne!(followed[0], followed[3]);
    assert_eq!(flows["versions"], json!([followed[0], followed[3]]));
    for name in ["a", "b", "c", "d"] {
        assert!(
            !board.exists(&format!("changes/{name}/flow.json")),
            "{name}"
        );
    }
    assert!(board.exists("changes/e/flow.json"));
    assert_eq!(board.read("changes/e.json")["flow"], "/changes/e/flow");
    assert_eq!(board.read("changes/a/runs/1.json")["flow"], followed[0]);
    // The run that named the removed copy names its version, and the run
    // that names the copy the migration keeps is left as it is.
    assert_eq!(board.read("changes/b/runs/1.json")["flow"], followed[1]);
    assert_eq!(
        board.read("changes/e/runs/1.json")["flow"],
        "/changes/e/flow"
    );
    for name in ["a", "b", "c", "d"] {
        assert_eq!(board.code(&format!("/changes/{name}")), None, "{name}");
    }
    assert_eq!(board.code("/changes/a/runs/1"), None);
    assert_eq!(board.code("/changes/b/runs/1"), None);
    // A second apply finds nothing to write.
    let again = board.call("migrate", &json!({"flows": true, "apply": true}));
    assert_eq!(
        (&again["flows"]["total"], &again["flows"]["versions"]),
        (&json!(0), &json!([]))
    );
    assert_eq!(board.board.head().unwrap().id, head.id);
}

/// Commits every file of `board` as it stands.
fn commit_all(board: &Changes) {
    let root = board.root().to_path_buf();
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "more"]);
}

#[test]
fn attempts_lists_each_old_run_writes_nothing_without_apply_and_apply_retypes_it_once() {
    let board = copies_board();
    let before = board.board.head().unwrap().id;
    // The run of a names its flow by name, which the new schema refuses, so
    // `attempts` alone leaves it for `flows`.
    let paths = json!(["/changes/b/runs/1", "/changes/e/runs/1"]);
    let dry = board.call("migrate", &json!({"attempts": true}));
    assert_eq!(dry["attempts"], paths);
    assert_eq!(dry["left"][0]["path"], "/changes/a/runs/1");
    assert_eq!(board.board.head().unwrap().id, before);
    assert_eq!(
        board.read("changes/b/runs/1.json")["$type"],
        "changes/run/v1"
    );
    let applied = board.call("migrate", &json!({"attempts": true, "apply": true}));
    assert_eq!(applied["attempts"], paths);
    let head = board.board.head().unwrap();
    assert_eq!(head.parents, [before]);
    assert!(
        head.message.ends_with("\n\nBoard-Bulk: migrate attempts"),
        "{}",
        head.message
    );
    for name in ["b", "e"] {
        let run = board.read(&format!("changes/{name}/runs/1.json"));
        assert_eq!(run["$type"], "changes/attempt/v1", "{name}");
        assert_eq!(
            board.code(&format!("/changes/{name}/runs/1")),
            None,
            "{name}"
        );
    }
    assert_eq!(
        board.read("changes/b/runs/1.json")["flow"],
        "/changes/b/flow"
    );
    let again = board.call("migrate", &json!({"attempts": true, "apply": true}));
    assert_eq!(again["attempts"], json!([]));
    assert_eq!(board.board.head().unwrap().id, head.id);
}

#[test]
fn an_old_run_that_fails_the_new_schema_is_left_and_the_run_beside_it_is_retyped() {
    let board = copies_board();
    let mut odd = board.read("changes/b/runs/1.json");
    odd["sandbox"] = json!("never");
    board.write("changes/b/runs/1.json", &odd.to_string());
    commit_all(&board);
    let dry = board.call("migrate", &json!({"attempts": true}));
    assert_eq!(dry["attempts"], json!(["/changes/e/runs/1"]));
    let left: Vec<&str> = dry["left"]
        .as_array()
        .unwrap()
        .iter()
        .map(|each| each["path"].as_str().unwrap())
        .collect();
    assert_eq!(left, ["/changes/a/runs/1", "/changes/b/runs/1"]);
    let why = dry["left"][1]["why"].as_str().unwrap();
    assert!(
        why.starts_with("its rewritten record at /changes/b/runs/1 fails its schema at"),
        "{why}"
    );
}

#[test]
fn flows_and_attempts_together_write_each_run_once_with_the_new_type_and_flow() {
    let board = copies_board();
    let output = board.call(
        "migrate",
        &json!({"flows": true, "attempts": true, "apply": true}),
    );
    assert_eq!(
        output["flows"]["runs"],
        json!(["/changes/a/runs/1", "/changes/b/runs/1"])
    );
    assert_eq!(
        output["attempts"],
        json!([
            "/changes/a/runs/1",
            "/changes/b/runs/1",
            "/changes/e/runs/1"
        ])
    );
    let head = board.board.head().unwrap();
    assert!(
        head.message.ends_with("\n\nBoard-Bulk: migrate flows"),
        "{}",
        head.message
    );
    let followed = board.read("changes/a.json")["flow"].clone();
    let run = board.read("changes/a/runs/1.json");
    assert_eq!(
        (&run["$type"], &run["flow"]),
        (&json!("changes/attempt/v1"), &followed)
    );
    assert_eq!(
        board.read("changes/e/runs/1.json")["$type"],
        "changes/attempt/v1"
    );
    assert_eq!(board.code("/changes/a/runs/1"), None);
}

#[test]
fn flows_and_attempts_together_list_a_run_naming_a_flow_its_change_does_not_follow_once() {
    let board = copies_board();
    let mut named = board.read("changes/e/runs/1.json");
    named["flow"] = json!("development");
    board.write("changes/e/runs/2.json", &named.to_string());
    commit_all(&board);
    let dry = board.call("migrate", &json!({"flows": true, "attempts": true}));
    let paths: Vec<&Value> = dry["flows"]["left"]
        .as_array()
        .unwrap()
        .iter()
        .map(|left| &left["path"])
        .filter(|path| *path == "/changes/e/runs/2")
        .collect();
    assert_eq!(paths.len(), 1, "{}", dry["flows"]["left"]);
}

#[test]
fn migrate_without_flows_leaves_the_copies_says_nothing_of_them_and_marks_its_commit() {
    let board = copies_board();
    board.write(
        "changes/old.json",
        &change("build", &json!({"records": [TASK]})),
    );
    commit_all(&board);
    let output = board.call("migrate", &json!({"apply": true}));
    assert_eq!(output.get("flows"), None);
    assert_eq!(output["changes"], json!(["/changes/old"]));
    assert!(board.exists("changes/a/flow.json"));
    assert_eq!(board.read("changes/a.json")["flow"], "/changes/a/flow");
    let head = board.board.head().unwrap();
    assert!(
        head.message.ends_with("\n\nBoard-Bulk: migrate changes"),
        "{}",
        head.message
    );
}

#[test]
fn a_change_whose_rewrite_fails_its_schema_is_left_and_the_rest_of_the_flows_migrate() {
    let board = copies_board();
    let mut odd = board.read("changes/c.json");
    odd["surfaced"] = json!("never");
    board.write("changes/c.json", &odd.to_string());
    commit_all(&board);
    let dry = board.call("migrate", &json!({"flows": true}));
    let flows = &dry["flows"];
    assert_eq!(
        flows["changes"],
        json!(["/changes/a", "/changes/b", "/changes/d"])
    );
    let left = &flows["left"][0];
    assert_eq!(left["path"], "/changes/c");
    let why = left["why"].as_str().unwrap();
    assert!(
        why.starts_with("its rewritten record at /changes/c fails its schema at"),
        "{why}"
    );
    assert_eq!(flows["copies"], 3);
    let applied = board.call("migrate", &json!({"flows": true, "apply": true}));
    assert_eq!(applied["flows"]["changes"], flows["changes"]);
    assert!(board.exists("changes/c/flow.json"));
    assert_eq!(board.read("changes/c.json")["flow"], "/changes/c/flow");
    assert!(!board.exists("changes/a/flow.json"));
}

#[test]
fn a_run_whose_rewrite_fails_its_schema_is_left_and_the_rest_of_the_flows_migrate() {
    let board = copies_board();
    let mut odd = board.read("changes/a/runs/1.json");
    odd["sandbox"] = json!("never");
    board.write("changes/a/runs/1.json", &odd.to_string());
    commit_all(&board);
    let dry = board.call("migrate", &json!({"flows": true}));
    let flows = &dry["flows"];
    assert_eq!(flows["runs"], json!(["/changes/b/runs/1"]));
    let left = &flows["left"][0];
    assert_eq!(left["path"], "/changes/a/runs/1");
    let why = left["why"].as_str().unwrap();
    assert!(
        why.starts_with("its rewritten record at /changes/a/runs/1 fails its schema at"),
        "{why}"
    );
    let applied = board.call("migrate", &json!({"flows": true, "apply": true}));
    assert_eq!(applied["flows"]["runs"], flows["runs"]);
    assert_eq!(board.read("changes/a/runs/1.json")["flow"], "development");
    assert!(!board.exists("changes/a/flow.json"));
}

#[test]
fn a_copy_stays_while_a_run_that_names_it_cannot_be_rewritten_and_one_with_a_rewritable_run_goes() {
    let board = copies_board();
    let mut odd = board.read("changes/b/runs/1.json");
    odd["sandbox"] = json!("never");
    board.write("changes/b/runs/1.json", &odd.to_string());
    commit_all(&board);
    let dry = board.call("migrate", &json!({"flows": true}));
    assert_eq!(dry["flows"]["copies"], 3);
    assert_eq!(dry["flows"]["left"][0]["path"], "/changes/b/runs/1");
    let applied = board.call("migrate", &json!({"flows": true, "apply": true}));
    assert_eq!(applied["flows"]["copies"], 3);
    assert!(board.exists("changes/b/flow.json"));
    assert_eq!(
        board.read("changes/b/runs/1.json")["flow"],
        "/changes/b/flow"
    );
    assert!(
        board.read("changes/b.json")["flow"]
            .as_str()
            .unwrap()
            .starts_with("/flows/")
    );
    // The run of a names its flow by name, and c has no run: their copies go.
    assert!(!board.exists("changes/a/flow.json"));
    assert!(!board.exists("changes/c/flow.json"));
}

#[test]
fn reflow_keeps_a_copy_a_run_names_and_removes_a_copy_no_run_names() {
    let board = copies_board();
    let flow = json!({"path": "/changes/b", "flow": "development"});
    board.call("reflow", &flow);
    assert!(
        board.read("changes/b.json")["flow"]
            .as_str()
            .unwrap()
            .starts_with("/flows/")
    );
    assert!(board.exists("changes/b/flow.json"));
    assert_eq!(
        board.read("changes/b/runs/1.json")["flow"],
        "/changes/b/flow"
    );
    assert_eq!(board.code("/changes/b/runs/1"), None);
    let flow = json!({"path": "/changes/c", "flow": "development"});
    board.call("reflow", &flow);
    assert!(!board.exists("changes/c/flow.json"));
}

const OWED_TEXT: &str = "# T\n\n## Problem\n\nA.\n\n### Considered\n\n- /notes/a.md: about the old flow.\n- `/notes/b.md` and `/notes/c.md`: both unrelated.\n- Unrelated: `/notes/d.md`, `/notes/a.md`.\n\nNothing here:\n/notes/b.md,\n/notes/c.md.\n\n## Obligations left owed\n\n- /controls/x/owed, owed to the next step.\n\n## Outcome\n\nB.\n";

/// A board committing each write, with a mount, the notes and control the
/// plans name, and these changes in `(name, phase, text)`.
fn plans_board(changes: &[(&str, &str, &str)]) -> Changes {
    plans_board_enabling(&["changes", "controls"], changes)
}

/// As [`plans_board`], with `plugins` enabled.
fn plans_board_enabling(plugins: &[&str], changes: &[(&str, &str, &str)]) -> Changes {
    let board = Changes::enabling(plugins);
    board.mount_x(&[("README.md", "x\n")]);
    let mut config = board.read(".board/board.json");
    config["commit_on_put"] = json!(true);
    board.write(".board/board.json", &config.to_string());
    let root = board.root().to_path_buf();
    git(&root, &["config", "user.name", "t"]);
    git(&root, &["config", "user.email", "t@t"]);
    for name in ["a", "b", "c", "d"] {
        board.write(&format!("notes/{name}.md"), "# N\n");
    }
    let owed = json!({
        "$type": "controls/control/v1", "kind": "obligation", "requirement": "Sweep.",
        "purpose": "p",
    });
    board.write("controls/x/owed.json", &owed.to_string());
    for (name, phase, text) in changes {
        board.write(&format!("changes/{name}/plans/1.md"), text);
        let plan = typed(
            "changes/plan/v1",
            &json!({"text": format!("/changes/{name}/plans/1.md"), "uses": ["/notes/a.md"], "paths": ["/repos/x/**"]}),
        );
        board.write(&format!("changes/{name}/plans/1.json"), &plan);
        let change = json!({
            "$type": "changes/change/v2", "title": format!("Change {name}"), "mount": "/repos/x",
            "flow": "/flows/development/none", "phase": phase, "surfaced": [], "task": TASK,
            "plan": format!("/changes/{name}/plans/1"), "touched": "2026-01-02",
        });
        board.write(&format!("changes/{name}.json"), &change.to_string());
    }
    commit_all(&board);
    board
}

/// The plan record the change at `name` now names, and its text.
fn current_plan(board: &Changes, name: &str) -> (Value, String) {
    let change = board.read(&format!("changes/{name}.json"));
    let path = change["plan"]
        .as_str()
        .unwrap()
        .trim_start_matches('/')
        .to_owned();
    let plan = board.read(&format!("{path}.json"));
    let text = board.text(plan["text"].as_str().unwrap().trim_start_matches('/'));
    (plan, text)
}

#[test]
fn migrate_with_plans_lists_what_it_would_write_and_writes_nothing_without_apply() {
    let board = plans_board(&[
        ("good", "plan", OWED_TEXT),
        ("over", "done", OWED_TEXT),
        ("plain", "plan", "# T\n\n## Problem\n\nA.\n"),
        (
            "odd",
            "plan",
            "# T\n\n### Considered\n\n- Using a Glob: rejected, because.\n",
        ),
    ]);
    let before = board.board.head().unwrap().id;
    let dry = board.call("migrate", &json!({"plans": true}));
    let plans = &dry["plan_fields"];
    assert_eq!(plans["applied"], false);
    assert_eq!(plans["changes"], json!(["/changes/good"]));
    assert_eq!(
        (&plans["total"], &plans["considered"], &plans["owed"]),
        (&json!(1), &json!(4), &json!(1))
    );
    assert_eq!(plans["unread"][0]["path"], "/changes/odd");
    assert_eq!(plans["unread"][0]["code"], "changes::migrate::unread");
    assert_eq!(
        plans["unread"][0]["line"],
        "- Using a Glob: rejected, because."
    );
    assert_eq!(board.board.head().unwrap().id, before);
    assert_eq!(
        board.read("changes/good.json")["plan"],
        "/changes/good/plans/1"
    );
}

#[test]
fn migrate_with_plans_writes_a_new_snapshot_in_one_bulk_commit_and_a_second_apply_writes_nothing() {
    let board = plans_board(&[
        ("good", "plan", OWED_TEXT),
        ("over", "done", OWED_TEXT),
        (
            "odd",
            "plan",
            "# T\n\n### Considered\n\n- Using a Glob: rejected, because.\n",
        ),
        (
            "gone",
            "plan",
            "# T\n\n### Considered\n\n- /notes/none.md: no longer there.\n",
        ),
    ]);
    let before = board.board.head().unwrap().id;
    let applied = board.call("migrate", &json!({"plans": true, "apply": true}));
    assert_eq!(applied["plan_fields"]["total"], 1);
    let head = board.board.head().unwrap();
    assert_eq!(head.parents, [before]);
    assert!(
        head.message.ends_with("\n\nBoard-Bulk: migrate plans"),
        "{}",
        head.message
    );
    let (plan, text) = current_plan(&board, "good");
    assert_eq!(
        plan["considered"],
        json!([
            {"records": ["/notes/a.md"], "why": "about the old flow."},
            {"records": ["/notes/b.md", "/notes/c.md"], "why": "both unrelated."},
            {"records": ["/notes/d.md", "/notes/a.md"], "why": "Unrelated"},
            {"records": ["/notes/b.md", "/notes/c.md"], "why": "Nothing here"},
        ])
    );
    assert_eq!(
        plan["owed"],
        json!([{"control": "/controls/x/owed", "why": "owed to the next step."}])
    );
    assert_eq!(plan["uses"], json!(["/notes/a.md"]));
    assert_eq!(plan["paths"], json!(["/repos/x/**"]));
    assert_eq!(text, "# T\n\n## Problem\n\nA.\n\n## Outcome\n\nB.\n");
    let change = board.read("changes/good.json");
    assert_eq!(
        change["touched"], "2026-01-02",
        "a migration is not an edit"
    );
    assert_eq!(board.code(change["plan"].as_str().unwrap()), None);
    assert_eq!(
        board.read("changes/over.json")["plan"],
        "/changes/over/plans/1"
    );
    assert_eq!(
        board.read("changes/odd.json")["plan"],
        "/changes/odd/plans/1"
    );
    assert_eq!(
        board.read("changes/gone.json")["plan"],
        "/changes/gone/plans/1"
    );
    let again = board.call("migrate", &json!({"plans": true, "apply": true}));
    assert_eq!(again["plan_fields"]["total"], 0);
    assert_eq!(board.board.head().unwrap().id, head.id);
}

const PROSE_EVAL: &str = "# T\n\n### Considered\n\n- /evals/prose: it tested something else.\n";
const GONE_NOTE: &str = "# T\n\n### Considered\n\n- /notes/none.md: no longer there.\n";

#[test]
fn migrate_with_plans_accepts_a_path_that_exists_but_is_degraded_and_refuses_one_that_reaches_nothing()
 {
    let board = plans_board_enabling(
        &["changes", "controls", "evals"],
        &[
            ("degraded", "plan", PROSE_EVAL),
            ("gone", "plan", GONE_NOTE),
        ],
    );
    let eval = json!({
        "$type": "evals/eval/v1", "question": "Q?", "subjects": ["o/r"], "status": "planned",
        "decision": "Which one.",
    });
    board.write("evals/prose.json", &eval.to_string());
    commit_all(&board);
    assert_eq!(
        board.code("/evals/prose").as_deref(),
        Some("board::ref::malformed")
    );
    let dry = board.call("migrate", &json!({"plans": true}));
    assert_eq!(dry["plan_fields"]["changes"], json!(["/changes/degraded"]));
    assert_eq!(dry["plan_fields"]["unread"][0]["path"], "/changes/gone");
    assert_eq!(dry["plan_fields"]["unread"][0]["line"], "/notes/none.md");
    board.call("migrate", &json!({"plans": true, "apply": true}));
    let (plan, _) = current_plan(&board, "degraded");
    assert_eq!(
        plan["considered"],
        json!([{"records": ["/evals/prose"], "why": "it tested something else."}])
    );
    assert_eq!(
        board.read("changes/gone.json")["plan"],
        "/changes/gone/plans/1"
    );
}

#[test]
fn migrate_with_plans_leaves_a_change_whose_owed_line_names_a_note_and_moves_one_that_names_a_control()
 {
    let board = plans_board(&[
        (
            "note",
            "plan",
            "# T\n\n## Obligations left owed\n\n- /notes/a.md, owed to the next step.\n",
        ),
        (
            "control",
            "plan",
            "# T\n\n## Obligations left owed\n\n- /controls/x/owed, owed to the next step.\n",
        ),
    ]);
    let output = board.call("migrate", &json!({"plans": true, "apply": true}));
    let plans = &output["plan_fields"];
    assert_eq!(plans["changes"], json!(["/changes/control"]));
    assert_eq!(plans["unread"][0]["path"], "/changes/note");
    assert_eq!(plans["unread"][0]["code"], "changes::migrate::unread");
    assert_eq!(plans["unread"][0]["line"], "/notes/a.md");
    assert_eq!(
        board.read("changes/note.json")["plan"],
        "/changes/note/plans/1"
    );
    let (plan, _) = current_plan(&board, "control");
    assert_eq!(
        plan["owed"],
        json!([{"control": "/controls/x/owed", "why": "owed to the next step."}])
    );
}

#[test]
fn migrate_with_plans_leaves_a_change_whose_new_snapshot_fails_its_schema_and_moves_its_twin() {
    let board = plans_board(&[
        (
            "empty-reason",
            "plan",
            "# T\n\n### Considered\n\n:\n/notes/a.md\n",
        ),
        (
            "reason",
            "plan",
            "# T\n\n### Considered\n\nUnrelated:\n/notes/a.md\n",
        ),
    ]);
    let output = board.call("migrate", &json!({"plans": true, "apply": true}));
    let plans = &output["plan_fields"];
    assert_eq!(plans["changes"], json!(["/changes/reason"]));
    assert_eq!(plans["unread"][0]["path"], "/changes/empty-reason");
    assert_eq!(plans["unread"][0]["code"], "changes::migrate::unread");
    assert_eq!(plans["unread"][0]["line"], "/changes/empty-reason/plans/1");
    assert!(
        plans["unread"][0]["why"]
            .as_str()
            .unwrap()
            .contains("rewritten record"),
        "{plans}"
    );
    assert_eq!(
        board.read("changes/empty-reason.json")["plan"],
        "/changes/empty-reason/plans/1"
    );
}

/// Writes the lifecycle record of the change `name` and points the plan at
/// `plans/1` to it.
fn with_lifecycle(board: &Changes, name: &str) -> String {
    let record = format!("/changes/{name}/lifecycle");
    board.write(&format!("changes/{name}/lifecycle.md"), "A table.\n");
    board.write(
        &format!("changes/{name}/lifecycle.json"),
        &typed(
            "changes/lifecycle/v1",
            &json!({"text": format!("{record}.md"), "doc": "/repos/x/docs/states.md"}),
        ),
    );
    let at = format!("changes/{name}/plans/1.json");
    let mut plan = board.read(&at);
    plan["lifecycle"] = json!(record);
    board.write(&at, &plan.to_string());
    record
}

#[test]
fn migrate_with_plans_keeps_the_plans_lifecycle_record_and_one_never_given_stays_unset() {
    let board = plans_board(&[("good", "plan", OWED_TEXT), ("bare", "plan", OWED_TEXT)]);
    let record = with_lifecycle(&board, "good");
    commit_all(&board);
    let applied = board.call("migrate", &json!({"plans": true, "apply": true}));
    assert_eq!(applied["plan_fields"]["total"], 2);
    assert_eq!(current_plan(&board, "good").0["lifecycle"], json!(record));
    assert_eq!(current_plan(&board, "bare").0.get("lifecycle"), None);
}

#[test]
fn migrating_a_v1_change_keeps_its_plans_lifecycle_record_and_one_never_given_stays_unset() {
    let board = Changes::new();
    board.mount_x(&[("README.md", "x\n")]);
    for name in ["named", "bare"] {
        board.write(&format!("changes/{name}/plans/1.md"), PLAN);
        board.write(
            &format!("changes/{name}/plans/1.json"),
            &typed(
                "changes/plan/v1",
                &json!({
                    "text": format!("/changes/{name}/plans/1.md"),
                    "uses": [],
                    "paths": ["/repos/x/**"],
                }),
            ),
        );
        board.write(
            &format!("changes/{name}.json"),
            &change(
                "build",
                &json!({"plan": format!("/changes/{name}/plans/1"), "records": [TASK]}),
            ),
        );
    }
    let record = with_lifecycle(&board, "named");
    let output = board.call("migrate", &json!({"apply": true}));
    assert_eq!(output["total"], 2);
    assert_eq!(current_plan(&board, "named").0["lifecycle"], json!(record));
    assert_eq!(current_plan(&board, "bare").0.get("lifecycle"), None);
}

/// A migration copies the plan record a v1 change names when it moves the
/// change's records onto it, and the copy keeps the plan's typed answers.
#[test]
fn a_migration_s_copy_of_a_plan_carries_the_plan_s_typed_answers() {
    let board = Changes::new();
    board.mount_x(&[("README.md", "x\n")]);
    board.write("changes/t/plans/1.md", PLAN);
    let given = json!({"ux": "none", "data": "stored", "lifecycle": "none"});
    board.write(
        "changes/t/plans/1.json",
        &typed(
            "changes/plan/v1",
            &json!({"text": "/changes/t/plans/1.md", "uses": [], "answers": given}),
        ),
    );
    board.write(
        "changes/t.json",
        &change(
            "build",
            &json!({"plan": "/changes/t/plans/1", "records": [TASK]}),
        ),
    );
    let output = board.call("migrate", &json!({"apply": true}));
    assert_eq!(output["changes"], json!(["/changes/t"]));
    assert_eq!(
        board.read("changes/t/plans/1.json"),
        json!({
            "$type": "changes/plan/v1",
            "text": "/changes/t/plans/1.md",
            "uses": [],
            "records": [TASK],
            "answers": given,
        })
    );
}
