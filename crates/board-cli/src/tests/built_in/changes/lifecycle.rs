//! `changes.lifecycle`, which writes a change's states, events and crash
//! points beside its plan, and the gates that read the record through real
//! controls.

use super::*;

const CHANGE: &str = "/changes/t";
const RECORD: &str = "changes/t/lifecycle.json";
const TEXT: &str = "changes/t/lifecycle.md";
/// The most words the lifecycle text may hold.
const LIFECYCLE_WORDS: usize = 1_500;
const TABLE: &str = "| State | Event | Next |\n| --- | --- | --- |\n| open | stop | closed |\n";
const NAMED: &str = "See the change's lifecycle record: open, closed.";

/// `count` words.
fn words(count: usize) -> String {
    "w ".repeat(count)
}

fn lifecycle(text: &str, doc: &str) -> Value {
    json!({"path": CHANGE, "text": text, "doc": doc, "external": ["GitHub pull requests"]})
}

/// A change in plan with a plan over `/repos/x/**`.
fn planned() -> Changes {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    board.mount_x(&[("src/sync.rs", "a\n")]);
    board.call_stdin(
        "plan",
        &json!({"path": CHANGE, "text": PLAN, "uses": [], "paths": PATHS}),
    );
    board
}

/// The record of the plan the change now names.
fn plan_record(board: &Changes) -> Value {
    let plan = board.read("changes/t.json")["plan"]
        .as_str()
        .unwrap()
        .trim_start_matches('/')
        .to_owned();
    board.read(&format!("{plan}.json"))
}

#[test]
fn lifecycle_writes_the_record_and_its_text_and_points_the_plan_at_them() {
    let board = planned();
    let output = board.call_stdin("lifecycle", &lifecycle(TABLE, "/repos/x/docs/states.md"));
    assert_eq!(
        output,
        json!({"path": "/changes/t/lifecycle", "text": "/changes/t/lifecycle.md"})
    );
    assert_eq!(
        board.read(RECORD),
        json!({
            "$type": "changes/lifecycle/v1",
            "text": "/changes/t/lifecycle.md",
            "doc": "/repos/x/docs/states.md",
            "external": ["GitHub pull requests"],
        })
    );
    assert_eq!(board.text(TEXT), TABLE);
    assert_eq!(plan_record(&board)["lifecycle"], "/changes/t/lifecycle");
    assert_eq!(board.code("/changes/t/lifecycle"), None);
}

#[test]
fn lifecycle_refuses_text_over_1500_words_and_admits_1500() {
    let board = planned();
    let refused = board.refusal_stdin(
        "lifecycle",
        &lifecycle(&words(LIFECYCLE_WORDS + 1), "/repos/x/docs/states.md"),
    );
    assert_eq!(refused["code"], "changes::lifecycle::too_long");
    assert_eq!(refused["fields"]["words"], LIFECYCLE_WORDS + 1);
    assert!(!board.exists(RECORD), "a refusal writes nothing");
    board.call_stdin(
        "lifecycle",
        &lifecycle(&words(LIFECYCLE_WORDS), "/repos/x/docs/states.md"),
    );
    assert!(board.exists(RECORD));
}

#[test]
fn lifecycle_refuses_a_doc_outside_the_mount_with_a_pattern_or_naming_no_file_and_admits_a_file() {
    let board = planned();
    let refused = board.refusal_stdin("lifecycle", &lifecycle(TABLE, "/repos/y/docs/states.md"));
    assert_eq!(refused["code"], "changes::lifecycle::doc_outside_mount");
    assert_eq!(refused["fields"]["doc"], "/repos/y/docs/states.md");
    let refused = board.refusal_stdin("lifecycle", &lifecycle(TABLE, "/repos/xy/docs/states.md"));
    assert_eq!(refused["code"], "changes::lifecycle::doc_outside_mount");
    for doc in [
        "/repos/x/../y/a.md",
        "/repos/x/./a.md",
        "/repos/x/docs/..",
        "/repos/x/docs/",
    ] {
        let refused = board.refusal_stdin("lifecycle", &lifecycle(TABLE, doc));
        assert_eq!(
            refused["code"], "changes::lifecycle::doc_not_a_file",
            "{doc}"
        );
        assert_eq!(refused["fields"]["doc"], doc);
    }
    let refused = board.refusal_stdin("lifecycle", &lifecycle(TABLE, "/repos/x/docs/*.md"));
    assert_eq!(refused["code"], "changes::lifecycle::doc_is_a_pattern");
    assert!(!board.exists(RECORD), "a refusal writes nothing");
    board.call_stdin("lifecycle", &lifecycle(TABLE, "/repos/x/docs/states.md"));
    assert!(board.exists(RECORD));
}

#[test]
fn lifecycle_refuses_a_change_with_no_plan_and_a_done_one_and_admits_one_with_a_plan() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    let refused = board.refusal_stdin("lifecycle", &lifecycle(TABLE, "/repos/x/docs/states.md"));
    assert_eq!(refused["code"], "changes::lifecycle::no_plan");
    assert!(!board.exists(RECORD), "a refusal writes nothing");
    let done = planned();
    done.change_at("changes/t.json", "done", &json!({}));
    let refused = done.refusal_stdin("lifecycle", &lifecycle(TABLE, "/repos/x/docs/states.md"));
    assert_eq!(refused["code"], "changes::phase::final");
    let build = planned();
    build.change_at("changes/t.json", "build", &json!({}));
    build.call_stdin("lifecycle", &lifecycle(TABLE, "/repos/x/docs/states.md"));
    assert!(build.exists(RECORD));
}

#[test]
fn a_second_lifecycle_replaces_the_first_and_the_same_one_again_changes_nothing() {
    let board = planned();
    board.call_stdin("lifecycle", &lifecycle(TABLE, "/repos/x/docs/states.md"));
    board.call_stdin(
        "lifecycle",
        &lifecycle("Changed.\n", "/repos/x/docs/states.md"),
    );
    assert_eq!(board.text(TEXT), "Changed.\n");
    let again = board.call_stdin(
        "lifecycle",
        &lifecycle("Changed.\n", "/repos/x/docs/states.md"),
    );
    assert_eq!(again["path"], "/changes/t/lifecycle");
    assert_eq!(board.text(TEXT), "Changed.\n");
}

#[test]
fn a_revised_plan_keeps_the_lifecycle_record_and_one_never_given_stays_unset() {
    let board = planned();
    assert_eq!(plan_record(&board).get("lifecycle"), None);
    board.call_stdin("lifecycle", &lifecycle(TABLE, "/repos/x/docs/states.md"));
    board.call_stdin(
        "plan",
        &json!({"path": CHANGE, "text": format!("{PLAN}More.\n"), "uses": [], "paths": PATHS}),
    );
    assert_eq!(plan_record(&board)["lifecycle"], "/changes/t/lifecycle");
}

/// The fixture with a change whose plan says `section` under Lifecycle over
/// `paths`, and with the lifecycle-paths control when `control` is set.
fn gated(enabled: &[&str], section: &str, paths: &[&str], control: bool) -> Changes {
    let board = Changes::enabling(enabled);
    board.change_at("changes/t.json", "plan", &json!({"route": "owner"}));
    board.mount_x(&[("src/sync.rs", "a\n"), ("src/a.rs", "a\n")]);
    if control {
        board.write(
            "controls/x/lifecycle-paths.json",
            &typed(
                "controls/control/v1",
                &json!({
                    "requirement": "This code changes state.",
                    "purpose": "p",
                    "paths": ["repos/x/src/sync.rs"],
                }),
            ),
        );
    }
    let (before, _) = PLAN.rsplit_once("## Lifecycle").unwrap();
    let text = format!("{before}## Lifecycle\n\n{section}\n");
    board.call_stdin(
        "plan",
        &json!({"path": CHANGE, "text": text, "uses": [], "paths": paths}),
    );
    board
}

fn check<'a>(next: &'a Value, id: &str) -> Option<&'a Value> {
    (next["gate"].as_array().unwrap().iter()).find(|each| each["check"] == id)
}

#[test]
fn a_plan_over_a_lifecycle_path_cannot_say_none_and_its_twins_may() {
    let enabled = ["changes", "controls"];
    let next = |board: &Changes| board.call("next", &json!({"path": CHANGE}));
    let none = "None: nothing changes state.";
    let reached = gated(&enabled, none, &["/repos/x/src/sync.rs"], true);
    let found = next(&reached);
    let declared = check(&found, "lifecycle-declared").unwrap();
    assert_eq!(declared["status"], "missing", "{declared}");
    assert_eq!(
        declared["detail"],
        "the plan's paths reach /controls/x/lifecycle-paths while its Lifecycle section declares \
         nothing"
    );
    assert!(
        declared["remedy"]
            .as_str()
            .unwrap()
            .contains("changes lifecycle"),
        "{declared}"
    );
    let pattern = gated(&enabled, none, &["/repos/x/**"], true);
    assert_eq!(
        check(&next(&pattern), "lifecycle-declared").unwrap()["status"],
        "missing"
    );
    let elsewhere = gated(&enabled, none, &["/repos/x/src/a.rs"], true);
    let found = next(&elsewhere);
    assert_eq!(
        check(&found, "lifecycle-declared").unwrap()["status"],
        "pass"
    );
    let named = gated(&enabled, NAMED, &["/repos/x/src/sync.rs"], true);
    let found = next(&named);
    assert!(check(&found, "lifecycle-declared").is_none(), "{found}");
    assert_eq!(
        check(&found, "lifecycle-record").unwrap()["status"],
        "missing"
    );
    named.call_stdin("lifecycle", &lifecycle(TABLE, "/repos/x/docs/states.md"));
    assert_eq!(
        check(&next(&named), "lifecycle-record").unwrap()["status"],
        "pass"
    );
}

#[test]
fn with_no_lifecycle_paths_control_none_passes_and_the_check_says_none_exists() {
    let said = "no control /controls/x/lifecycle-paths exists, so the board names no such paths \
                and the check passes";
    let none = "None: nothing changes state.";
    for enabled in [&["changes", "controls"][..], &["changes"][..]] {
        let board = gated(enabled, none, &["/repos/x/src/sync.rs"], false);
        let found = board.call("next", &json!({"path": CHANGE}));
        let declared = check(&found, "lifecycle-declared").unwrap();
        assert_eq!(declared["status"], "pass", "{enabled:?}: {declared}");
        assert_eq!(declared["detail"], said, "{enabled:?}");
    }
}

/// A change started on `flow` whose head edits `file`, in a board with a
/// lifecycle-paths control that lists src/sync.rs, and that head.
fn started_over(flow: &str, file: &str) -> (Changes, String) {
    let board = Changes::enabling(&["changes", "controls"]);
    let (_, head) = board.edited(&[(file, "a\n")]);
    board.write(
        "controls/x/lifecycle-paths.json",
        &typed(
            "controls/control/v1",
            &json!({
                "requirement": "This code changes state.",
                "purpose": "p",
                "paths": ["repos/x/src/sync.rs"],
            }),
        ),
    );
    board.call(
        "start",
        &json!({"mount": "/repos/x", "task": TASK, "title": "T", "name": "t", "flow": flow}),
    );
    (board, head)
}

/// Moves the change to `phase` with `head` recorded.
fn moved(board: &Changes, phase: &str, head: &str) {
    let mut change = board.read("changes/t.json");
    change["phase"] = json!(phase);
    change["head"] = json!(head);
    board.write("changes/t.json", &change.to_string());
}

/// A change started on the `small` flow, reflowed to `development` at build
/// and moved to review, whose head edits a file a lifecycle-paths control
/// lists. Its plan has no Lifecycle heading.
fn reflowed_from_small() -> Changes {
    let (board, head) = started_over("small", "src/sync.rs");
    board.call("reflow", &json!({"path": CHANGE, "flow": "development"}));
    moved(&board, "review", &head);
    board
}

/// The input of `plan` for the fixture plan with `section` under Lifecycle.
fn plan_saying(section: &str) -> Value {
    let (before, _) = PLAN.rsplit_once("## Lifecycle").unwrap();
    let text = format!("{before}## Lifecycle\n\n{section}\n");
    json!({"path": CHANGE, "text": text, "uses": [], "paths": PATHS})
}

/// The status of check `id` in the change's gate, none when it is left out.
fn status_of(board: &Changes, id: &str) -> Option<Value> {
    let found = board.call("next", &json!({"path": CHANGE}));
    check(&found, id).map(|each| each["status"].clone())
}

#[test]
fn a_plan_revised_to_none_with_a_record_misses_review_and_a_plan_naming_it_does_not() {
    let board = reflowed_from_small();
    board.call_stdin("lifecycle", &lifecycle(TABLE, "/repos/x/docs/states.md"));
    board.call_stdin("plan", &plan_saying("None: nothing changes state."));
    assert_eq!(
        status_of(&board, "lifecycle-none-paths"),
        Some(json!("missing"))
    );
    board.call_stdin("plan", &plan_saying(NAMED));
    assert_eq!(status_of(&board, "lifecycle-none-paths"), None);
    assert_eq!(status_of(&board, "lifecycle-paths"), None);
}

#[test]
fn only_a_lifecycle_record_board_reads_at_the_path_switches_off_lifecycle_paths() {
    let understanding = typed(
        "changes/understanding/v1",
        &json!({
            "change": CHANGE,
            "subject": "data",
            "playback": "One record per call.",
            "questions": [{"round": 1, "question": "Q?", "answer": "A.", "matches": true}],
        }),
    );
    let invalid = typed("changes/lifecycle/v1", &json!({}));
    let cases = [
        ("changes/t/lifecycle/x.md", "text\n"),
        (RECORD, understanding.as_str()),
        (RECORD, "{}"),
        (RECORD, "not json"),
        (RECORD, invalid.as_str()),
    ];
    for (file, body) in cases {
        let board = reflowed_from_small();
        board.write(file, body);
        assert_eq!(
            status_of(&board, "lifecycle-paths"),
            Some(json!("missing")),
            "{file}: {body}"
        );
    }
    let board = reflowed_from_small();
    board.call_stdin("lifecycle", &lifecycle(TABLE, "/repos/x/docs/states.md"));
    assert_eq!(status_of(&board, "lifecycle-paths"), None);
}

#[test]
fn a_small_change_in_build_over_a_lifecycle_path_misses_not_small_and_one_elsewhere_passes() {
    for (file, want) in [("src/sync.rs", "missing"), ("src/a.rs", "pass")] {
        let (board, head) = started_over("small", file);
        moved(&board, "build", &head);
        assert_eq!(status_of(&board, "not-small"), Some(json!(want)), "{file}");
    }
}

#[test]
fn a_small_change_in_review_over_a_lifecycle_path_misses_and_one_elsewhere_passes() {
    for (file, want) in [("src/sync.rs", "missing"), ("src/a.rs", "pass")] {
        let (board, head) = started_over("small", file);
        moved(&board, "review", &head);
        assert_eq!(
            status_of(&board, "lifecycle-paths"),
            Some(json!(want)),
            "{file}"
        );
    }
}

#[test]
fn a_change_in_review_reflowed_to_small_over_a_lifecycle_path_misses_and_one_elsewhere_passes() {
    for (file, want) in [("src/sync.rs", "missing"), ("src/a.rs", "pass")] {
        let (board, head) = started_over("development", file);
        moved(&board, "review", &head);
        board.call("reflow", &json!({"path": CHANGE, "flow": "small"}));
        assert_eq!(
            status_of(&board, "lifecycle-paths"),
            Some(json!(want)),
            "{file}"
        );
    }
}

#[test]
fn a_change_reflowed_from_small_at_build_faces_lifecycle_paths_until_it_has_a_record() {
    let board = reflowed_from_small();
    let next = |board: &Changes| board.call("next", &json!({"path": CHANGE}));
    let found = next(&board);
    let paths = check(&found, "lifecycle-paths").unwrap();
    assert_eq!(paths["status"], "missing", "{paths}");
    assert_eq!(
        paths["detail"],
        "the change's diff edits files that /controls/x/lifecycle-paths list: /repos/x/src/sync.rs"
    );
    board.call_stdin("lifecycle", &lifecycle(TABLE, "/repos/x/docs/states.md"));
    let found = next(&board);
    assert!(check(&found, "lifecycle-paths").is_none(), "{found}");
}

/// The shipped plan phase also has the `obligations` check, which reads the
/// same capability and propagates its refusal, so `next` refuses there.
#[test]
fn next_on_the_shipped_flow_refuses_with_the_controls_refusal_when_for_paths_refuses() {
    let enabled = ["changes", "controls"];
    let none = "None: nothing changes state.";
    let refusing = gated(&enabled, none, &["/repos/x/../y"], true);
    let refused = refusing.refusal("next", &json!({"path": CHANGE}));
    assert_eq!(refused["code"], "changes::board::refused");
    assert_eq!(refused["fields"]["code"], "controls::board::refused");
    let reading = gated(&enabled, none, &["/repos/x/src/a.rs"], true);
    let found = reading.call("next", &json!({"path": CHANGE}));
    assert_eq!(
        check(&found, "lifecycle-declared").unwrap()["status"],
        "pass"
    );
}
