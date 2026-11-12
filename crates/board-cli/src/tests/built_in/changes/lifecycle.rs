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
        &json!({"path": CHANGE, "text": PLAN, "uses": [], "paths": PATHS, "answers": answers()}),
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
        &json!({"path": CHANGE, "text": format!("{PLAN}More.\n"), "uses": [], "paths": PATHS, "answers": answers()}),
    );
    assert_eq!(plan_record(&board)["lifecycle"], "/changes/t/lifecycle");
}

/// The lifecycle-paths rule, listing src/sync.rs, of `rule_type` at
/// `<prefix>/x/lifecycle-paths`.
fn lifecycle_rule(board: &Changes, prefix: &str, rule_type: &str) {
    board.write(
        &format!("{prefix}/x/lifecycle-paths.json"),
        &typed(
            rule_type,
            &json!({
                "requirement": "This code changes state.",
                "purpose": "p",
                "paths": ["repos/x/src/sync.rs"],
            }),
        ),
    );
}

/// The fixture with a change whose plan says `section` under Lifecycle over
/// `paths`, and with the lifecycle-paths rule when `rule` is set.
fn gated(enabled: &[&str], section: &str, paths: &[&str], rule: bool) -> Changes {
    let board = Changes::enabling(enabled);
    board.change_at("changes/t.json", "plan", &json!({"route": "owner"}));
    board.mount_x(&[("src/sync.rs", "a\n"), ("src/a.rs", "a\n")]);
    if rule {
        lifecycle_rule(&board, "rules", "rules/rule/v1");
    }
    let (before, _) = PLAN.rsplit_once("## Lifecycle").unwrap();
    let text = format!("{before}## Lifecycle\n\n{section}\n");
    board.call_stdin(
        "plan",
        &json!({"path": CHANGE, "text": text, "uses": [], "paths": paths, "answers": answering(section)}),
    );
    board
}

/// The typed answers of the fixture plan with `section` under Lifecycle:
/// `lifecycle` is none when the section starts with None, and record
/// otherwise.
fn answering(section: &str) -> Value {
    let lifecycle = if section.starts_with("None") {
        "none"
    } else {
        "record"
    };
    json!({"ux": "none", "data": "none", "lifecycle": lifecycle})
}

fn check<'a>(next: &'a Value, id: &str) -> Option<&'a Value> {
    (next["gate"].as_array().unwrap().iter()).find(|each| each["check"] == id)
}

#[test]
fn a_plan_over_a_lifecycle_path_cannot_say_none_and_its_twins_may() {
    let enabled = ["changes", "rules"];
    let next = |board: &Changes| board.call("next", &json!({"path": CHANGE}));
    let none = "None: nothing changes state.";
    let reached = gated(&enabled, none, &["/repos/x/src/sync.rs"], true);
    let found = next(&reached);
    let declared = check(&found, "lifecycle-declared").unwrap();
    assert_eq!(declared["status"], "missing", "{declared}");
    assert_eq!(
        declared["detail"],
        "the plan's paths reach /rules/x/lifecycle-paths"
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
fn with_no_lifecycle_paths_rule_none_passes_and_the_check_says_none_exists() {
    let said = "no rule /rules/x/lifecycle-paths exists, so the board names no such paths \
                and the check passes";
    let none = "None: nothing changes state.";
    for enabled in [&["changes", "rules"][..], &["changes"][..]] {
        let board = gated(enabled, none, &["/repos/x/src/sync.rs"], false);
        let found = board.call("next", &json!({"path": CHANGE}));
        let declared = check(&found, "lifecycle-declared").unwrap();
        assert_eq!(declared["status"], "pass", "{enabled:?}: {declared}");
        assert_eq!(declared["detail"], said, "{enabled:?}");
    }
}

/// A change started on `flow` whose head edits `file`, in a board with a
/// lifecycle-paths rule that lists src/sync.rs, and that head.
fn started_over(flow: &str, file: &str) -> (Changes, String) {
    let board = Changes::enabling(&["changes", "rules"]);
    let (_, head) = board.edited(&[(file, "a\n")]);
    lifecycle_rule(&board, "rules", "rules/rule/v1");
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
    json!({"path": CHANGE, "text": text, "uses": [], "paths": PATHS, "answers": answering(section)})
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
    let dangling = typed(
        "changes/lifecycle/v1",
        &json!({"text": "/changes/t/lifecycle.md", "doc": "/repos/x/docs/states.md"}),
    );
    let cases = [
        ("changes/t/lifecycle/x.md", "text\n"),
        (RECORD, understanding.as_str()),
        (RECORD, "{}"),
        (RECORD, "not json"),
        (RECORD, invalid.as_str()),
        (RECORD, dangling.as_str()),
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
        "the change's diff edits files that /rules/x/lifecycle-paths list: /repos/x/src/sync.rs"
    );
    board.call_stdin("lifecycle", &lifecycle(TABLE, "/repos/x/docs/states.md"));
    let found = next(&board);
    assert!(check(&found, "lifecycle-paths").is_none(), "{found}");
}

/// The gate of a change in `phase` whose head edits src/sync.rs and whose
/// plan says None under Lifecycle, with the lifecycle-paths rule of
/// `rule_type` under `prefix`, on the shipped development flow or, when
/// `stored` is set, on its version stored before the rename, which names
/// `controls` under `/controls/`.
fn lifecycle_gate(stored: bool, prefix: &str, rule_type: &str, phase: &str) -> Value {
    let (board, head) = lifecycle_change(stored, prefix, rule_type, phase);
    gate_naming(&board, &head)
}

/// The board of [`lifecycle_gate`] and its head.
fn lifecycle_change(stored: bool, prefix: &str, rule_type: &str, phase: &str) -> (Changes, String) {
    let board = Changes::enabling(&["changes", "rules"]);
    let (_, head) = board.edited(&[("src/sync.rs", "a\n")]);
    lifecycle_rule(&board, prefix, rule_type);
    board.call(
        "start",
        &json!({"mount": "/repos/x", "task": TASK, "title": "T", "name": "t", "flow": "development"}),
    );
    if stored {
        let old = include_str!(
            "../../../../../board-plugin-changes/src/flow/flows-md/development-a9df0910bd037be9.json"
        );
        board.write("flows/development/a9df0910bd037be9.json", old);
        let mut change = board.read("changes/t.json");
        change["flow"] = json!("/flows/development/a9df0910bd037be9");
        board.write("changes/t.json", &change.to_string());
    }
    let mut plan = plan_saying("None: nothing changes state.");
    plan["paths"] = json!(["/repos/x/src/sync.rs"]);
    board.call_stdin("plan", &plan);
    moved(&board, phase, &head);
    (board, head)
}

/// The gate of the change at [`CHANGE`]. Each board commits its own head
/// and stamps its own plan, so the gate names them as HEAD and PLAN.
fn gate_naming(board: &Changes, head: &str) -> Value {
    let plan = board.read("changes/t.json")["plan"]
        .as_str()
        .unwrap()
        .to_owned();
    let gate = board.call("next", &json!({"path": CHANGE}))["gate"].to_string();
    serde_json::from_str(&gate.replace(head, "HEAD").replace(&plan, "PLAN")).unwrap()
}

#[test]
fn renaming_stores_an_old_flow_again_under_the_new_names_and_its_change_gates_as_before() {
    const OLD: &str = "/flows/development/a9df0910bd037be9";
    for phase in ["plan", "land"] {
        let (board, head) = lifecycle_change(true, "controls", "controls/control/v1", phase);
        let before = gate_naming(&board, &head);
        let renamed = board.run(&["--json", "call", "rules.migrate", "--rename", "true"], "");
        let digest = json(&renamed)["data"]["output"]["digest"]
            .as_str()
            .unwrap()
            .to_owned();
        let applied = board.run(
            &[
                "--json",
                "call",
                "rules.migrate",
                "--rename",
                "true",
                "--apply",
                "true",
                "--digest",
                &digest,
            ],
            "",
        );
        assert_eq!(applied.code, 0, "{}", text(&applied.stdout));
        let flow = board.read("changes/t.json")["flow"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(
            flow.starts_with("/flows/development/") && flow != OLD,
            "{flow}"
        );
        let stored = board.read(&format!("{}.json", &flow[1..])).to_string();
        assert!(
            !stored.contains("\"controls\"")
                && !stored.contains("/controls/")
                && !stored.contains("controlled")
                && stored.contains("\"rule-paths\""),
            "{stored}"
        );
        assert!(!board.exists(&format!("{}.json", &OLD[1..])), "{phase}");
        assert!(board.exists("rules/x/lifecycle-paths.json"), "{phase}");
        assert_eq!(gate_naming(&board, &head), before, "{phase}");
    }
}

#[test]
fn two_old_flow_versions_that_rename_alike_are_stored_once_and_a_flow_off_its_hash_is_kept() {
    const OLD: &str = "/flows/development/a9df0910bd037be9";
    const OFF: &str = "/flows/development/0000000000000000";
    let (board, _) = lifecycle_change(true, "controls", "controls/control/v1", "plan");
    let old = board.read(&format!("{}.json", &OLD[1..]));
    // The same flow with `rules` in place of `controls`, as a flow stored
    // between the two renames holds it, which renames to the same JSON.
    let mut twin = old.clone();
    for check in (twin["phases"].as_array_mut().unwrap().iter_mut())
        .flat_map(|phase| phase["checks"].as_array_mut().unwrap())
    {
        if let Some(rules) = check.as_object_mut().unwrap().remove("controls") {
            check["rules"] = rules;
        }
    }
    let twin_path = board_model::flow_version("development", &twin);
    assert_ne!(twin_path, OLD);
    board.write(&format!("{}.json", &twin_path[1..]), &twin.to_string());
    board.write("notes/n.md", &format!("Pinned to {twin_path}.\n"));
    board.write(&format!("{}.json", &OFF[1..]), &old.to_string());
    let rename = |flags: &[&str]| {
        let arguments: Vec<&str> = ["--json", "call", "rules.migrate", "--rename", "true"]
            .into_iter()
            .chain(flags.iter().copied())
            .collect();
        board.run(&arguments, "")
    };
    let listed = json(&rename(&[]))["data"]["output"].clone();
    let kept: Vec<&str> = (listed["kept"].as_array().unwrap().iter())
        .filter_map(|each| each["path"].as_str())
        .collect();
    assert!(kept.contains(&OFF), "{listed}");
    let digest = listed["digest"].as_str().unwrap();
    let applied = rename(&["--apply", "true", "--digest", digest]);
    assert_eq!(applied.code, 0, "{}", text(&applied.stdout));
    let flow = board.read("changes/t.json")["flow"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(board.text("notes/n.md"), format!("Pinned to {flow}.\n"));
    for gone in [OLD, twin_path.as_str()] {
        assert!(!board.exists(&format!("{}.json", &gone[1..])), "{gone}");
    }
    assert_eq!(board.read(&format!("{}.json", &OFF[1..])), old);
}

#[test]
fn a_change_on_a_flow_stored_before_the_rename_gates_as_one_on_the_flow_with_the_new_names() {
    for phase in ["plan", "review", "land"] {
        let now = lifecycle_gate(false, "rules", "rules/rule/v1", phase);
        let lifecycle_checks: Vec<&Value> = (now.as_array().unwrap().iter())
            .filter(|each| each["check"].as_str().unwrap().starts_with("lifecycle"))
            .collect();
        assert!(
            !lifecycle_checks.is_empty()
                && lifecycle_checks
                    .iter()
                    .all(|each| each["status"] == "missing"),
            "{phase}: {now}"
        );
        for (stored, prefix, rule_type) in [
            (true, "rules", "rules/rule/v1"),
            (false, "controls", "controls/control/v1"),
            (true, "controls", "controls/control/v1"),
        ] {
            assert_eq!(
                lifecycle_gate(stored, prefix, rule_type, phase),
                now,
                "{phase} {stored} {prefix}"
            );
        }
    }
}

/// The shipped plan phase also has the `obligations` check, which reads the
/// same capability and propagates its refusal, so `next` refuses there.
#[test]
fn next_on_the_shipped_flow_refuses_with_the_controls_refusal_when_for_paths_refuses() {
    let enabled = ["changes", "rules"];
    let none = "None: nothing changes state.";
    let refusing = gated(&enabled, none, &["/repos/x/../y"], true);
    let refused = refusing.refusal("next", &json!({"path": CHANGE}));
    assert_eq!(refused["code"], "changes::board::refused");
    assert_eq!(refused["fields"]["code"], "rules::board::refused");
    let reading = gated(&enabled, none, &["/repos/x/src/a.rs"], true);
    let found = reading.call("next", &json!({"path": CHANGE}));
    assert_eq!(
        check(&found, "lifecycle-declared").unwrap()["status"],
        "pass"
    );
}

#[test]
fn a_development_change_in_land_over_a_lifecycle_path_with_no_record_misses_and_its_twins_pass() {
    let (edited, head) = started_over("development", "src/sync.rs");
    moved(&edited, "land", &head);
    assert_eq!(
        status_of(&edited, "lifecycle-paths-at-land"),
        Some(json!("missing"))
    );
    edited.call_stdin("lifecycle", &lifecycle(TABLE, "/repos/x/docs/states.md"));
    assert_eq!(status_of(&edited, "lifecycle-paths-at-land"), None);
    let (elsewhere, head) = started_over("development", "src/a.rs");
    moved(&elsewhere, "land", &head);
    assert_eq!(
        status_of(&elsewhere, "lifecycle-paths-at-land"),
        Some(json!("pass"))
    );
}

#[test]
fn a_development_change_in_land_saying_none_over_a_lifecycle_path_misses_and_one_naming_passes() {
    let (board, head) = started_over("development", "src/sync.rs");
    board.call_stdin("plan", &plan_saying("None: nothing changes state."));
    board.call_stdin("lifecycle", &lifecycle(TABLE, "/repos/x/docs/states.md"));
    moved(&board, "land", &head);
    assert_eq!(
        status_of(&board, "lifecycle-none-paths-at-land"),
        Some(json!("missing"))
    );
    board.call_stdin("plan", &plan_saying(NAMED));
    assert_eq!(status_of(&board, "lifecycle-none-paths-at-land"), None);
}

#[test]
fn a_small_change_in_land_over_a_lifecycle_path_misses_and_one_elsewhere_passes() {
    for (file, want) in [("src/sync.rs", "missing"), ("src/a.rs", "pass")] {
        let (board, head) = started_over("small", file);
        moved(&board, "land", &head);
        assert_eq!(
            status_of(&board, "lifecycle-paths-at-land"),
            Some(json!(want)),
            "{file}"
        );
    }
}

#[test]
fn a_revised_plan_drops_a_lifecycle_reference_board_no_longer_reads_and_keeps_one_it_does() {
    let board = planned();
    board.call_stdin("lifecycle", &lifecycle(TABLE, "/repos/x/docs/states.md"));
    let revise = |more: &str| {
        board.call_stdin(
            "plan",
            &json!({"path": CHANGE, "text": format!("{PLAN}{more}\n"), "uses": [], "paths": PATHS, "answers": answers()}),
        );
    };
    revise("Kept.");
    assert_eq!(plan_record(&board)["lifecycle"], "/changes/t/lifecycle");
    board.delete(RECORD);
    revise("Gone.");
    assert_eq!(plan_record(&board).get("lifecycle"), None);
}

#[test]
fn the_same_plan_resubmitted_after_its_lifecycle_record_is_removed_drops_the_reference() {
    let board = planned();
    let same =
        json!({"path": CHANGE, "text": PLAN, "uses": [], "paths": PATHS, "answers": answers()});
    board.call_stdin("lifecycle", &lifecycle(TABLE, "/repos/x/docs/states.md"));
    assert_eq!(board.call_stdin("plan", &same)["unchanged"], true);
    assert_eq!(plan_record(&board)["lifecycle"], "/changes/t/lifecycle");
    board.delete(RECORD);
    assert_eq!(board.call_stdin("plan", &same)["unchanged"], false);
    assert_eq!(plan_record(&board).get("lifecycle"), None);
}

#[test]
fn lifecycle_refuses_a_doc_with_whitespace_or_a_placeholder_and_admits_a_path() {
    let board = planned();
    for doc in ["/repos/x/docs/a b.md", "/repos/x/docs/{name}.md"] {
        let refused = board.refusal_stdin("lifecycle", &lifecycle(TABLE, doc));
        assert_eq!(refused["code"], "board::plugin::bad_input", "{doc}");
        assert!(!board.exists(RECORD), "{doc}: a refusal writes nothing");
    }
    board.call_stdin("lifecycle", &lifecycle(TABLE, "/repos/x/docs/states.md"));
    assert!(board.exists(RECORD));
}

/// The lifecycle record is the second of the four lines one `put` carries,
/// and a directory at its path makes `put` refuse that line. The text line
/// before it stays unwritten only because the four lines are one write.
#[test]
fn a_lifecycle_put_with_a_refused_line_after_the_text_writes_no_line_and_its_twin_writes_them() {
    let board = planned();
    let before = board.text("changes/t.json");
    board.write("changes/t/lifecycle.json/x.txt", "x\n");
    let refused = board.refusal_stdin("lifecycle", &lifecycle(TABLE, "/repos/x/docs/states.md"));
    assert_eq!(refused["code"], "changes::board::refused", "{refused}");
    assert!(
        refused["message"].as_str().unwrap().contains("put --batch"),
        "{refused}"
    );
    assert!(!board.exists(TEXT));
    assert!(board.root().join(RECORD).is_dir());
    assert_eq!(plan_record(&board).get("lifecycle"), None);
    assert_eq!(board.text("changes/t.json"), before);
    let twin = planned();
    twin.call_stdin("lifecycle", &lifecycle(TABLE, "/repos/x/docs/states.md"));
    assert!(twin.exists(TEXT));
    assert!(twin.exists(RECORD));
    assert_eq!(plan_record(&twin)["lifecycle"], "/changes/t/lifecycle");
}

#[test]
fn next_on_a_plan_phase_says_what_board_changes_lifecycle_reads_on_stdin_and_build_does_not() {
    let board = planned();
    let said = board.call("next", &json!({"path": CHANGE}))["instruction"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(
        said.contains(
            "Pipe `board changes lifecycle` JSON on stdin with the fields `path`, `text`, `doc` \
             and `external`"
        ),
        "{said}"
    );
    assert!(
        said.contains(r#"{"path":"/changes/t","text":"<text>""#),
        "{said}"
    );
    let build = planned();
    build.change_at("changes/t.json", "build", &json!({}));
    let said = build.call("next", &json!({"path": CHANGE}))["instruction"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(!said.contains("Pipe `board changes lifecycle`"), "{said}");
}
