//! `test_plan.py`: `changes.plan`, which writes a change's plan and the
//! records it uses, within the limits on both.

use super::*;

const CHANGE: &str = "/changes/t";
/// The words a plan may hold.
const PLAN_WORDS: usize = 3_000;
/// The words the documents a plan uses may hold in all.
const USES_WORDS: usize = 3_000;

/// A Markdown document of `count` words under one heading word.
fn words(count: usize) -> String {
    format!("#{}\n", " w".repeat(count))
}

/// The input of `changes.plan` for [`CHANGE`] with `text` and `uses`,
/// declaring [`PATHS`].
fn given(text: &str, uses: &[&str]) -> Value {
    json!({"path": CHANGE, "text": text, "uses": uses, "paths": PATHS})
}

/// The files under the change's plans directory.
fn plans(board: &Changes) -> usize {
    std::fs::read_dir(board.root().join("changes/t/plans")).map_or(0, Iterator::count)
}

/// The plan record a `changes.plan` output names, read from the board.
fn stored(board: &Changes, output: &Value) -> Value {
    let path = output["path"].as_str().unwrap().trim_start_matches('/');
    board.read(&format!("{path}.json"))
}

#[test]
fn plan_writes_the_plan_and_its_text_and_points_the_change_at_the_plan() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    board.write("standards/errors.md", "# Errors\n");
    let output = board.call_stdin("plan", &given(PLAN, &["/standards/errors.md"]));
    let stamp = output["path"]
        .as_str()
        .unwrap()
        .strip_prefix("/changes/t/plans/")
        .unwrap()
        .to_owned();
    assert_eq!(
        output,
        json!({
            "path": format!("/changes/t/plans/{stamp}"),
            "text": format!("/changes/t/plans/{stamp}.md"),
            "uses": 1,
            "unchanged": false,
            "warnings": [],
        })
    );
    assert_eq!(
        board.read(&format!("changes/t/plans/{stamp}.json")),
        json!({
            "$type": "changes/plan/v1",
            "text": format!("/changes/t/plans/{stamp}.md"),
            "uses": ["/standards/errors.md"],
            "paths": PATHS,
        })
    );
    assert_eq!(board.text(&format!("changes/t/plans/{stamp}.md")), PLAN);
    assert_eq!(
        board.read("changes/t.json")["plan"],
        format!("/changes/t/plans/{stamp}")
    );
}

#[test]
fn a_second_plan_is_written_beside_the_first_and_becomes_the_change_s_plan() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    let first = board.call_stdin("plan", &given(PLAN, &[]));
    let second = board.call_stdin("plan", &given(&format!("{PLAN}More.\n"), &[]));
    assert!(
        first["path"].as_str().unwrap() < second["path"].as_str().unwrap(),
        "{first} {second}"
    );
    // Two plans, each a record and its text.
    assert_eq!(plans(&board), 2 * 2);
    assert_eq!(board.read("changes/t.json")["plan"], second["path"]);
}

#[test]
fn a_plan_that_uses_a_path_holding_nothing_is_refused_and_writes_nothing() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    board.write("standards/errors.md", "# Errors\n");
    let refused = board.refusal_stdin(
        "plan",
        &given(PLAN, &["/standards/errors.md", "/standards/none"]),
    );
    assert_eq!(refused["code"], "changes::ref::dangling");
    assert_eq!(refused["fields"]["refs"], json!(["/standards/none"]));
    assert!(!board.exists("changes/t/plans"));
    let output = board.call_stdin("plan", &given(PLAN, &["/standards/errors.md"]));
    assert_eq!(output["uses"], 1);
}

#[test]
fn a_plan_over_3000_words_is_refused_and_one_of_3000_is_written() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    let refused = board.refusal_stdin("plan", &given(&words(PLAN_WORDS), &[]));
    assert_eq!(refused["code"], "changes::plan::too_long");
    assert_eq!(refused["fields"]["words"], PLAN_WORDS + 1);
    let output = board.call_stdin("plan", &given(&words(PLAN_WORDS - 1), &[]));
    assert_eq!(output["uses"], 0);
}

#[test]
fn documents_used_over_3000_words_in_all_are_refused_and_3000_are_admitted() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    board.write("standards/a.md", &words(1499));
    board.write("standards/b.md", &words(1500));
    board.write(
        "decisions/d.json",
        &typed("plans/decision/v1", &json!({"decision": " w".repeat(4000)})),
    );
    let uses = ["/standards/a.md", "/standards/b.md", "/decisions/d"];
    let refused = board.refusal_stdin("plan", &given(PLAN, &uses));
    assert_eq!(refused["code"], "changes::plan::uses_too_long");
    assert_eq!(refused["fields"]["words"], USES_WORDS + 1);
    board.write("standards/b.md", &words(1499));
    let output = board.call_stdin("plan", &given(PLAN, &uses));
    assert_eq!(output["uses"], uses.len());
}

#[test]
fn a_plan_for_a_done_change_is_refused_and_one_for_a_change_in_build_is_written() {
    let board = Changes::new();
    board.change_at("changes/t.json", "done", &json!({}));
    assert_eq!(
        board.refusal_stdin("plan", &given(PLAN, &[]))["code"],
        "changes::phase::final"
    );
    board.change_at("changes/t.json", "build", &json!({}));
    assert_eq!(board.call_stdin("plan", &given(PLAN, &[]))["uses"], 0);
}

#[test]
fn a_plan_that_uses_a_directory_is_refused_and_one_using_a_document_in_it_is_written() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    board.write("standards/errors.md", "# Errors\n");
    let refused = board.refusal_stdin("plan", &given(PLAN, &["/standards"]));
    assert_eq!(
        (&refused["code"], &refused["fields"]["refs"]),
        (&json!("changes::plan::directory"), &json!(["/standards"]))
    );
    let output = board.call_stdin("plan", &given(PLAN, &["/standards/errors.md"]));
    assert_eq!(output["uses"], 1);
}

#[test]
fn a_used_record_that_fails_its_schema_is_refused_and_one_that_keeps_it_is_written() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    board.write(
        ".board/types/x/thing/v1.schema.json",
        r#"{"type": "object", "required": ["a"]}"#,
    );
    board.write("things/t.json", &typed("x/thing/v1", &json!({})));
    let refused = board.refusal_stdin("plan", &given(PLAN, &["/things/t"]));
    assert_eq!(
        (&refused["code"], &refused["fields"]["refs"]),
        (&json!("changes::ref::dangling"), &json!(["/things/t"]))
    );
    board.write("things/t.json", &typed("x/thing/v1", &json!({"a": 1})));
    assert_eq!(
        board.call_stdin("plan", &given(PLAN, &["/things/t"]))["uses"],
        1
    );
}

#[test]
fn a_reference_given_twice_is_kept_once() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    board.write("standards/errors.md", "# Errors\n");
    let output = board.call_stdin(
        "plan",
        &given(PLAN, &["/standards/errors.md", "/standards/errors.md"]),
    );
    assert_eq!(
        stored(&board, &output)["uses"],
        json!(["/standards/errors.md"])
    );
}

#[test]
fn a_plan_of_only_whitespace_is_refused_and_one_word_is_written() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    let refused = board.refusal_stdin("plan", &given("  \n ", &[]));
    assert_eq!(refused["code"], "board::plugin::bad_input");
    assert_eq!(board.call_stdin("plan", &given("Plan.", &[]))["uses"], 0);
}

#[test]
fn a_used_record_degraded_only_by_a_broken_reference_of_its_own_is_written() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    board.write(
        ".board/types/x/link/v1.schema.json",
        r#"{"type": "object", "properties": {"to": {"format": "board-ref"}}}"#,
    );
    board.write(
        "links/l.json",
        &typed("x/link/v1", &json!({"to": "/missing"})),
    );
    assert_eq!(
        board.call_stdin("plan", &given(PLAN, &["/links/l"]))["uses"],
        1
    );
}

#[test]
fn a_plan_for_a_build_change_without_paths_is_refused_and_one_with_paths_is_written() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    for unplanned in [
        json!({"path": CHANGE, "text": PLAN, "uses": []}),
        json!({"path": CHANGE, "text": PLAN, "uses": [], "paths": []}),
    ] {
        let refused = board.refusal_stdin("plan", &unplanned);
        assert_eq!(refused["code"], "changes::plan::no_paths", "{refused}");
    }
    assert!(!board.exists("changes/t/plans"));
    let paths = ["/repos/x/src/**", "/repos/x/README.md", "/repos/x/src/**"];
    let output = board.call_stdin("plan", &plan_input(CHANGE, &paths));
    assert_eq!(
        stored(&board, &output)["paths"],
        json!(["/repos/x/src/**", "/repos/x/README.md"])
    );
}

#[test]
fn a_planned_path_outside_the_change_s_mount_is_refused_and_one_inside_it_is_written() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    for outside in ["/repos/xy/src/**", "/repos/**"] {
        let refused = board.refusal_stdin("plan", &plan_input(CHANGE, &[outside]));
        assert_eq!(refused["code"], "changes::plan::outside_mount", "{outside}");
        assert_eq!(refused["fields"]["paths"], json!([outside]));
    }
    let output = board.call_stdin("plan", &plan_input(CHANGE, &["/repos/x/*"]));
    assert!(!output.as_object().unwrap().is_empty(), "{output}");
}

#[test]
fn a_plan_record_whose_path_does_not_start_at_the_root_is_invalid_and_one_that_does_is_valid() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    let output = board.call_stdin("plan", &given(PLAN, &[]));
    let logical = output["path"].as_str().unwrap();
    let file = format!("{}.json", logical.trim_start_matches('/'));
    let written = board.read(&file);
    board.write(
        &file,
        &record(written.clone(), &json!({"paths": ["repos/x/**"]})),
    );
    assert_eq!(
        board.code(logical).as_deref(),
        Some("board::record::invalid")
    );
    board.write(&file, &written.to_string());
    assert_eq!(board.code(logical), None);
}

#[test]
fn a_revised_plan_keeps_the_records_attached_to_the_plan_it_replaces_in_order() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    let first = board.call_stdin("plan", &given(PLAN, &[]));
    board.write("r/1.json", &report(HEAD, "approve", BASE, &json!({})));
    let approved = board.approved();
    board.call("attach", &json!({"path": CHANGE, "ref": approved}));
    board.call("attach", &json!({"path": CHANGE, "ref": "/r/1"}));
    let earlier = stored(&board, &first);
    let second = board.call_stdin("plan", &given(&format!("{PLAN}More.\n"), &[]));
    assert_eq!(
        stored(&board, &second)["records"],
        json!([APPROVED, "/r/1"])
    );
    // The earlier plan record is left as it was.
    assert_eq!(stored(&board, &first), earlier);
    // A later attach adds to the end of the copied list.
    board.write("r/2.json", &report(HEAD, "approve", BASE, &json!({})));
    board.call("attach", &json!({"path": CHANGE, "ref": "/r/2"}));
    assert_eq!(
        stored(&board, &second)["records"],
        json!([APPROVED, "/r/1", "/r/2"])
    );
}

#[test]
fn a_revised_plan_of_a_change_whose_plan_had_no_records_has_none() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    board.call_stdin("plan", &given(PLAN, &[]));
    let second = board.call_stdin("plan", &given(&format!("{PLAN}More.\n"), &[]));
    let written = stored(&board, &second);
    assert_eq!(written.get("records"), None, "{written}");
}

#[test]
fn the_same_plan_twice_writes_one_snapshot_and_a_plan_with_another_path_writes_a_second() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    board.write("standards/errors.md", "# Errors\n");
    let first = board.call_stdin("plan", &given(PLAN, &["/standards/errors.md"]));
    let written = plans(&board);
    let again = board.call_stdin("plan", &given(PLAN, &["/standards/errors.md"]));
    assert_eq!(
        again,
        json!({
            "path": first["path"],
            "text": first["text"],
            "uses": 1,
            "unchanged": true,
            "warnings": [],
        })
    );
    assert_eq!(plans(&board), written);
    assert_eq!(board.read("changes/t.json")["plan"], first["path"]);
    let mut other = given(PLAN, &["/standards/errors.md"]);
    other["paths"] = json!(["/repos/x/src/**"]);
    let second = board.call_stdin("plan", &other);
    assert_eq!(second["unchanged"], false);
    assert_ne!(second["path"], first["path"]);
    assert_eq!(plans(&board), written + 2);
}

#[test]
fn a_plan_using_a_file_beneath_a_mount_that_cannot_be_read_is_refused_naming_the_mount() {
    let plan_using = |board: &Changes| {
        let mut input = plan_input(CHANGE, &["/repos/x/src/a.rs"]);
        input["uses"] = json!(["/repos/x/README.md"]);
        board.change_at("changes/t.json", "plan", &json!({}));
        input
    };
    let board = Changes::new();
    board.mount_x(&[("README.md", "x\n")]);
    let input = plan_using(&board);
    board.call_stdin("plan", &input);
    board.break_mount("repos/x", true);
    let refused = board.refusal_stdin("plan", &input);
    assert_eq!(refused["code"], "changes::board::refused", "{refused}");
    assert_eq!(refused["fields"]["code"], "board::mount::repo_missing");
    assert!(
        refused["message"].as_str().unwrap().contains("/repos/x"),
        "{refused}"
    );
}

/// [`given`] with `extra` beside its fields.
fn with(text: &str, extra: &Value) -> Value {
    let mut input = given(text, &[]);
    for (name, value) in extra.as_object().unwrap() {
        input[name] = value.clone();
    }
    input
}

#[test]
fn a_considered_record_that_reaches_nothing_is_refused_and_one_that_is_there_is_written() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    let list = |path: &str| json!({"considered": [{"records": [path], "why": "It is about the old flow."}]});
    let refused = board.refusal_stdin("plan", &with(PLAN, &list("/notes/gone.md")));
    assert_eq!(refused["code"], "changes::ref::dangling");
    assert_eq!(plans(&board), 0);
    board.write("notes/there.md", "# There\n");
    let output = board.call_stdin("plan", &with(PLAN, &list("/notes/there.md")));
    assert_eq!(
        stored(&board, &output)["considered"],
        list("/notes/there.md")["considered"]
    );
    assert_eq!(output["warnings"], json!([]));
}

/// An obligation at `/controls/x/owed`, as `controls.check` would list it.
fn owed_control(board: &Changes) {
    let owed = json!({
        "$type": "controls/control/v1", "kind": "obligation", "requirement": "Sweep the docs.",
        "purpose": "p", "paths": ["repos/x/**"],
    });
    board.write("controls/x/owed.json", &owed.to_string());
}

#[test]
fn an_owed_control_that_is_a_task_is_refused_and_one_that_is_an_obligation_is_written() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    owed_control(&board);
    let owe = |path: &str| json!({"owed": [{"control": path, "why": "The next step does it."}]});
    let refused = board.refusal_stdin("plan", &with(PLAN, &owe(TASK)));
    assert_eq!(refused["code"], "changes::ref::wrong_type");
    assert_eq!(plans(&board), 0);
    let gone = board.refusal_stdin("plan", &with(PLAN, &owe("/controls/x/gone")));
    assert_eq!(gone["code"], "changes::ref::dangling");
    let output = board.call_stdin("plan", &with(PLAN, &owe("/controls/x/owed")));
    assert_eq!(
        stored(&board, &output)["owed"],
        owe("/controls/x/owed")["owed"]
    );
}

#[test]
fn a_plan_stays_valid_when_its_owed_control_is_deleted_and_is_dangling_when_a_considered_record_is()
{
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    owed_control(&board);
    board.write("notes/there.md", "# There\n");
    let fields = json!({
        "owed": [{"control": "/controls/x/owed", "why": "The next step does it."}],
        "considered": [{"records": ["/notes/there.md"], "why": "It is about the old flow."}],
    });
    let output = board.call_stdin("plan", &with(PLAN, &fields));
    let plan = output["path"].as_str().unwrap();
    assert_eq!(board.code(plan), None);
    board.delete("controls/x/owed.json");
    assert_eq!(board.code(plan), None);
    board.delete("notes/there.md");
    assert_eq!(board.code(plan).as_deref(), Some("board::ref::dangling"));
}

#[test]
fn a_plan_whose_text_holds_a_considered_or_owed_section_warns_and_one_without_does_not() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    let text = format!("{PLAN}\n### Considered\n\n- /tasks/a: unrelated.\n");
    let warned = board.call_stdin("plan", &given(&text, &[]));
    assert_eq!(
        warned["warnings"][0]["code"],
        "changes::plan::considered_in_text"
    );
    assert!(
        warned["warnings"][0]["detail"]
            .as_str()
            .unwrap()
            .contains("Considered"),
        "{warned}"
    );
    let owed = format!("{PLAN}\n## Obligations left owed\n\n- /controls/x: later.\n");
    let warned = board.call_stdin("plan", &given(&owed, &[]));
    assert_eq!(
        warned["warnings"][0]["code"],
        "changes::plan::considered_in_text"
    );
    let quiet = board.call_stdin("plan", &given(PLAN, &[]));
    assert_eq!(quiet["warnings"], json!([]));
}

#[test]
fn a_plan_with_other_considered_or_owed_fields_is_not_unchanged() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    board.write("notes/there.md", "# There\n");
    owed_control(&board);
    let considered = json!({"considered": [{"records": ["/notes/there.md"], "why": "Other."}]});
    let owed = json!({"owed": [{"control": "/controls/x/owed", "why": "Later."}]});
    let first = board.call_stdin("plan", &with(PLAN, &considered));
    assert_eq!(first["unchanged"], false);
    let same = board.call_stdin("plan", &with(PLAN, &considered));
    assert_eq!(same["unchanged"], true);
    std::thread::sleep(std::time::Duration::from_millis(2));
    let other = board.call_stdin("plan", &with(PLAN, &owed));
    assert_eq!(other["unchanged"], false);
}
