//! `test_plan.py`: `changes.plan`, which writes a change's plan and the
//! records it uses, within the limits on both.

use super::*;

const CHANGE: &str = "/changes/t";
/// The words a plan holds before it warns.
const PLAN_WORDS: usize = 3_000;
/// The words a plan may hold before it is refused.
const PLAN_CEILING: usize = 6_000;
/// The fixture plan with the two test sections the development flow's land
/// check reads, so a plan written from it warns of no missing section.
const WHOLE: &str = "# Let a suite declare a timeout\n\n## Problem\n\nSuites hang.\n\n## Outcome\n\nA suite stops.\n\n## Out of scope\n\nRetries.\n\n## UX\n\nNo UI change: suites run headless.\n\n## Data\n\nNone.\n\n## Failure paths\n\nNone: nothing can fail.\n\n## Consumers\n\nNone: nothing reads it.\n\n## Lifecycle\n\nNone: nothing changes state.\n\n## What the tests prove\n\nA suite stops.\n\n## What the tests do not prove\n\nThat suites are fast.\n";

/// A Markdown document of `count` words under one heading word.
fn words(count: usize) -> String {
    format!("#{}\n", " w".repeat(count))
}

/// The codes of the warnings in a `changes.plan` output, in order.
fn codes(output: &Value) -> Vec<&str> {
    (output["warnings"].as_array().unwrap().iter())
        .map(|warning| warning["code"].as_str().unwrap())
        .collect()
}

/// The detail of the warning `code` in a `changes.plan` output.
fn detail<'a>(output: &'a Value, code: &str) -> &'a str {
    (output["warnings"].as_array().unwrap().iter())
        .find(|warning| warning["code"] == code)
        .unwrap_or_else(|| panic!("no {code} in {output}"))["detail"]
        .as_str()
        .unwrap()
}

/// The input of `changes.plan` for [`CHANGE`] with `text` and `uses`,
/// declaring [`PATHS`].
fn given(text: &str, uses: &[&str]) -> Value {
    json!({"path": CHANGE, "text": text, "uses": uses, "paths": PATHS, "answers": answers()})
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
    let output = board.call_stdin("plan", &given(WHOLE, &["/standards/errors.md"]));
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
            "answers": answers(),
        })
    );
    assert_eq!(board.text(&format!("changes/t/plans/{stamp}.md")), WHOLE);
    assert_eq!(
        board.read("changes/t.json")["plan"],
        format!("/changes/t/plans/{stamp}")
    );
}

#[test]
fn a_second_plan_is_written_beside_the_first_and_becomes_the_change_s_plan() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    let first = board.call_stdin("plan", &given(WHOLE, &[]));
    let second = board.call_stdin("plan", &given(&format!("{WHOLE}More.\n"), &[]));
    assert!(
        first["path"].as_str().unwrap() < second["path"].as_str().unwrap(),
        "{first} {second}"
    );
    // Two plans, each a record and its text.
    assert_eq!(plans(&board), 2 * 2);
    assert_eq!(board.read("changes/t.json")["plan"], second["path"]);
}

#[test]
fn a_plan_that_uses_a_path_holding_nothing_is_written_without_it_and_warns_naming_it() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    board.write("standards/errors.md", "# Errors\n");
    let output = board.call_stdin(
        "plan",
        &given(WHOLE, &["/standards/errors.md", "/standards/none"]),
    );
    assert_eq!(codes(&output), ["changes::ref::dangling"], "{output}");
    assert!(
        detail(&output, "changes::ref::dangling").contains("/standards/none"),
        "{output}"
    );
    assert_eq!(output["uses"], 1);
    assert_eq!(
        stored(&board, &output)["uses"],
        json!(["/standards/errors.md"])
    );
    let again = board.call_stdin("plan", &given(WHOLE, &["/standards/errors.md"]));
    assert_eq!(again["unchanged"], true, "{again}");
    assert_eq!(again["warnings"], json!([]));
}

/// A plan over 3,000 words is written and warns with its count; one over
/// 6,000 is refused and writes nothing.
#[test]
fn a_plan_over_3000_words_is_written_with_a_warning_and_one_over_6000_is_refused() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    let long = format!("{WHOLE}\n{}", " w".repeat(3_500 - words_in(WHOLE)));
    let output = board.call_stdin("plan", &given(&long, &[]));
    assert_eq!(codes(&output), ["changes::plan::too_long"], "{output}");
    assert!(
        detail(&output, "changes::plan::too_long").contains("3,500 words"),
        "{output}"
    );
    assert!(board.exists(&format!(
        "{}.md",
        output["path"].as_str().unwrap().trim_start_matches('/')
    )));
    let refused = board.refusal_stdin("plan", &given(&words(6_499), &[]));
    assert_eq!(refused["code"], "changes::plan::too_long");
    assert_eq!(refused["fields"]["words"], 6_500);
    let edge = board.refusal_stdin("plan", &given(&words(PLAN_CEILING), &[]));
    assert_eq!(edge["fields"]["words"], PLAN_CEILING + 1);
    assert_eq!(plans(&board), 2);
    let quiet = board.call_stdin(
        "plan",
        &given(
            &format!("{WHOLE}\n{}", " w".repeat(PLAN_WORDS - words_in(WHOLE))),
            &[],
        ),
    );
    assert_eq!(quiet["warnings"], json!([]), "{quiet}");
}

/// How many whitespace-separated words `text` holds.
fn words_in(text: &str) -> usize {
    text.split_whitespace().count()
}

#[test]
fn documents_used_over_3000_words_in_all_are_written_with_a_warning_and_3000_are_quiet() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    board.write("standards/a.md", &words(1499));
    board.write("standards/b.md", &words(1500));
    board.write(
        "decisions/d.json",
        &typed("plans/decision/v1", &json!({"decision": " w".repeat(4000)})),
    );
    let uses = ["/standards/a.md", "/standards/b.md", "/decisions/d"];
    let warned = board.call_stdin("plan", &given(WHOLE, &uses));
    assert_eq!(codes(&warned), ["changes::plan::uses_too_long"], "{warned}");
    assert!(
        detail(&warned, "changes::plan::uses_too_long").contains("3,001 words"),
        "{warned}"
    );
    assert_eq!(warned["uses"], uses.len());
    board.write("standards/b.md", &words(1499));
    let output = board.call_stdin("plan", &given(WHOLE, &uses));
    assert_eq!(output["uses"], uses.len());
    assert_eq!(output["warnings"], json!([]));
}

/// A development-flow plan lacking the land phase's test sections warns at
/// plan time, naming both.
#[test]
fn a_plan_without_the_test_sections_is_written_and_warns_naming_both() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    let output = board.call_stdin("plan", &given(PLAN, &[]));
    assert_eq!(
        codes(&output),
        ["changes::plan::sections_missing"],
        "{output}"
    );
    let said = detail(&output, "changes::plan::sections_missing");
    assert!(
        said.contains("What the tests prove, What the tests do not prove"),
        "{said}"
    );
    assert!(!said.contains("Problem"), "{said}");
    let bare = board.call_stdin("plan", &given("## Problem\n\nSuites hang.\n", &[]));
    let said = detail(&bare, "changes::plan::sections_missing");
    assert!(said.contains("Outcome"), "{said}");
    assert!(said.contains("What the tests prove"), "{said}");
}

#[test]
fn a_plan_for_a_done_change_is_refused_and_one_for_a_change_in_build_is_written() {
    let board = Changes::new();
    board.change_at("changes/t.json", "done", &json!({}));
    assert_eq!(
        board.refusal_stdin("plan", &given(WHOLE, &[]))["code"],
        "changes::phase::final"
    );
    board.change_at("changes/t.json", "build", &json!({}));
    assert_eq!(board.call_stdin("plan", &given(WHOLE, &[]))["uses"], 0);
}

/// A done change takes a corrected plan only when the input says so, and
/// stays done.
#[test]
fn a_done_change_s_plan_is_corrected_with_correct_and_the_change_stays_done() {
    let board = Changes::new();
    board.change_at(
        "changes/t.json",
        "done",
        &json!({"plan": "/changes/t/plans/1"}),
    );
    let refused = board.refusal_stdin("plan", &given(WHOLE, &[]));
    assert_eq!(refused["code"], "changes::phase::final");
    assert!(
        refused["message"].as_str().unwrap().ends_with(
            "Only when the text is this done change's own plan, corrected, send it again with \
             \"correct\": true"
        ),
        "{refused}"
    );
    assert_eq!(board.read("changes/t.json")["plan"], "/changes/t/plans/1");
    let corrected = board.call_stdin("plan", &with(WHOLE, &json!({"correct": true})));
    assert_eq!(corrected["unchanged"], false);
    let held = board.read("changes/t.json");
    assert_eq!(held["plan"], corrected["path"]);
    assert_eq!(held["phase"], "done");
    let dropped = Changes::new();
    dropped.change_at("changes/t.json", "dropped", &json!({}));
    let output = dropped.call_stdin("plan", &with(WHOLE, &json!({"correct": true})));
    assert_eq!(dropped.read("changes/t.json")["phase"], "dropped");
    assert_eq!(dropped.read("changes/t.json")["plan"], output["path"]);
}

/// The text of another change's plan, headed by that change's title.
const OTHER_PLAN: &str = "# Bouncer looks up IP locations\n\n## Problem\n\nLookups fail.\n";

/// Writes the change /changes/bouncer-looks-up-ip-locations, the name
/// `changes.start` gives the title [`OTHER_PLAN`] is headed by, in `phase`.
fn other_change(board: &Changes, phase: &str) {
    board.write(
        "changes/bouncer-looks-up-ip-locations.json",
        &change(phase, &json!({"title": "Bouncer looks up IP locations"})),
    );
}

#[test]
fn a_plan_headed_by_another_change_s_title_is_refused_and_writes_nothing() {
    let board = Changes::new();
    board.change_at("changes/t.json", "build", &json!({}));
    other_change(&board, "build");
    let refused = board.refusal_stdin("plan", &given(OTHER_PLAN, &[]));
    assert_eq!(refused["code"], "changes::plan::other_change", "{refused}");
    assert_eq!(
        refused["fields"]["other"],
        "/changes/bouncer-looks-up-ip-locations"
    );
    assert!(!board.exists("changes/t/plans"));
    // A done change's correction is refused the same way.
    board.change_at("changes/t.json", "done", &json!({}));
    let mut correcting = given(OTHER_PLAN, &[]);
    correcting["correct"] = json!(true);
    let refused = board.refusal_stdin("plan", &correcting);
    assert_eq!(refused["code"], "changes::plan::other_change", "{refused}");
}

/// A live change at the name the heading derives whose title differs from
/// the heading, as after `--name` or a retitle, is not the plan's change.
#[test]
fn a_plan_headed_like_a_live_change_with_another_title_is_written_with_a_warning() {
    let board = Changes::new();
    board.change_at("changes/t.json", "build", &json!({}));
    board.write(
        "changes/bouncer-looks-up-ip-locations.json",
        &change(
            "build",
            &json!({"title": "Bouncer looks up IP locations through a cache"}),
        ),
    );
    let written = board.call_stdin("plan", &given(OTHER_PLAN, &[]));
    assert_eq!(written["unchanged"], false, "{written}");
    assert_eq!(
        written["warnings"][0]["code"], "changes::plan::title_differs",
        "{written}"
    );
    assert!(board.exists("changes/t/plans"));
}

#[test]
fn a_plan_headed_by_a_dropped_change_s_title_or_by_no_change_s_title_is_written_with_a_warning() {
    let board = Changes::new();
    board.change_at("changes/t.json", "build", &json!({}));
    let unknown = board.call_stdin("plan", &given(OTHER_PLAN, &[]));
    assert_eq!(
        unknown["warnings"][0]["code"], "changes::plan::title_differs",
        "{unknown}"
    );
    assert!(
        unknown["warnings"][0]["detail"]
            .as_str()
            .unwrap()
            .contains("Let a suite declare a timeout"),
        "{unknown}"
    );
    other_change(&board, "dropped");
    let dropped = board.call_stdin("plan", &given(&format!("{OTHER_PLAN}\nMore.\n"), &[]));
    assert_eq!(dropped["unchanged"], false);
    assert_eq!(
        dropped["warnings"][0]["code"],
        "changes::plan::title_differs"
    );
}

#[test]
fn a_plan_headed_by_its_own_title_or_with_no_heading_does_not_warn() {
    let board = Changes::new();
    board.change_at("changes/t.json", "build", &json!({}));
    let headed = board.call_stdin("plan", &given(WHOLE, &[]));
    assert_eq!(headed["warnings"], json!([]));
    let bare = board.call_stdin("plan", &given(WHOLE.split_once('\n').unwrap().1, &[]));
    assert_eq!(bare["warnings"], json!([]));
}

#[test]
fn a_plan_that_uses_a_directory_is_refused_and_one_using_a_document_in_it_is_written() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    board.write("standards/errors.md", "# Errors\n");
    let refused = board.refusal_stdin("plan", &given(WHOLE, &["/standards"]));
    assert_eq!(
        (&refused["code"], &refused["fields"]["refs"]),
        (&json!("changes::plan::directory"), &json!(["/standards"]))
    );
    let output = board.call_stdin("plan", &given(WHOLE, &["/standards/errors.md"]));
    assert_eq!(output["uses"], 1);
}

#[test]
fn a_used_record_that_fails_its_schema_is_left_out_with_a_warning_and_one_that_keeps_it_is_used() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    board.write(
        ".board/types/x/thing/v1.schema.json",
        r#"{"type": "object", "required": ["a"]}"#,
    );
    board.write("things/t.json", &typed("x/thing/v1", &json!({})));
    let warned = board.call_stdin("plan", &given(WHOLE, &["/things/t"]));
    assert_eq!(codes(&warned), ["changes::ref::dangling"], "{warned}");
    assert!(
        detail(&warned, "changes::ref::dangling").contains("/things/t"),
        "{warned}"
    );
    assert_eq!(warned["uses"], 0);
    board.write("things/t.json", &typed("x/thing/v1", &json!({"a": 1})));
    assert_eq!(
        board.call_stdin("plan", &given(WHOLE, &["/things/t"]))["uses"],
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
        &given(WHOLE, &["/standards/errors.md", "/standards/errors.md"]),
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
        board.call_stdin("plan", &given(WHOLE, &["/links/l"]))["uses"],
        1
    );
}

#[test]
fn a_plan_for_a_build_change_without_paths_is_refused_and_one_with_paths_is_written() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    for unplanned in [
        json!({"path": CHANGE, "text": WHOLE, "uses": []}),
        json!({"path": CHANGE, "text": WHOLE, "uses": [], "paths": []}),
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
    let output = board.call_stdin("plan", &given(WHOLE, &[]));
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
    let first = board.call_stdin("plan", &given(WHOLE, &[]));
    board.write("r/1.json", &report(HEAD, "approve", BASE, &json!({})));
    let approved = board.approved();
    board.call("attach", &json!({"path": CHANGE, "ref": approved}));
    board.call("attach", &json!({"path": CHANGE, "ref": "/r/1"}));
    let earlier = stored(&board, &first);
    let second = board.call_stdin("plan", &given(&format!("{WHOLE}More.\n"), &[]));
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
    board.call_stdin("plan", &given(WHOLE, &[]));
    let second = board.call_stdin("plan", &given(&format!("{WHOLE}More.\n"), &[]));
    let written = stored(&board, &second);
    assert_eq!(written.get("records"), None, "{written}");
}

#[test]
fn the_same_plan_twice_writes_one_snapshot_and_a_plan_with_another_path_writes_a_second() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    board.write("standards/errors.md", "# Errors\n");
    let first = board.call_stdin("plan", &given(WHOLE, &["/standards/errors.md"]));
    let written = plans(&board);
    let again = board.call_stdin("plan", &given(WHOLE, &["/standards/errors.md"]));
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
    let mut other = given(WHOLE, &["/standards/errors.md"]);
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
fn a_considered_record_that_reaches_nothing_is_left_out_with_a_warning_and_one_that_is_there_is_kept()
 {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    let list = |path: &str| json!({"considered": [{"records": [path], "why": "It is about the old flow."}]});
    let warned = board.call_stdin("plan", &with(WHOLE, &list("/notes/gone.md")));
    assert_eq!(codes(&warned), ["changes::ref::dangling"], "{warned}");
    assert!(
        detail(&warned, "changes::ref::dangling").contains("/notes/gone.md"),
        "{warned}"
    );
    // A group whose every record is left out is left out.
    assert_eq!(stored(&board, &warned).get("considered"), None);
    board.write("notes/there.md", "# There\n");
    let output = board.call_stdin("plan", &with(WHOLE, &list("/notes/there.md")));
    assert_eq!(
        stored(&board, &output)["considered"],
        list("/notes/there.md")["considered"]
    );
    assert_eq!(output["warnings"], json!([]));
}

/// A plan that considers a mistyped path in place of a record the change
/// surfaced is written without it, and `next` then reports the surfaced
/// record as not considered.
#[test]
fn a_considered_path_left_out_as_dangling_leaves_the_considered_check_missing() {
    let board = Changes::new();
    board.mount_x(&[("README.md", "x\n")]);
    let started =
        json!({"mount": "/repos/x", "task": TASK, "title": "Let a suite declare a timeout"});
    board.call("start", &started);
    board.write("notes/there.md", "# There\n");
    let mut change = board.read("changes/let-a-suite-declare-a-timeout.json");
    change["surfaced"] = json!(["/notes/there.md"]);
    board.write(
        "changes/let-a-suite-declare-a-timeout.json",
        &change.to_string(),
    );
    let mistyped = json!({"considered": [{"records": ["/notes/thre.md"], "why": "Unrelated."}]});
    let mut input = with(WHOLE, &mistyped);
    input["path"] = json!("/changes/let-a-suite-declare-a-timeout");
    let output = board.call_stdin("plan", &input);
    assert_eq!(codes(&output), ["changes::ref::dangling"], "{output}");
    assert!(
        detail(&output, "changes::ref::dangling").contains("/notes/thre.md"),
        "{output}"
    );
    let next = board.call(
        "next",
        &json!({"path": "/changes/let-a-suite-declare-a-timeout"}),
    );
    let considered = (next["gate"].as_array().unwrap().iter())
        .find(|check| check["check"] == "considered")
        .unwrap_or_else(|| panic!("no considered check in {next}"));
    assert_eq!(considered["status"], "missing", "{next}");
    assert!(
        considered["detail"]
            .as_str()
            .unwrap()
            .contains("/notes/there.md"),
        "{next}"
    );
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
fn an_owed_rule_that_is_a_task_is_refused_and_one_that_is_an_obligation_is_written() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    owed_control(&board);
    let owe = |path: &str| json!({"owed": [{"rule": path, "why": "The next step does it."}]});
    let refused = board.refusal_stdin("plan", &with(WHOLE, &owe(TASK)));
    assert_eq!(refused["code"], "changes::ref::wrong_type");
    assert_eq!(plans(&board), 0);
    let gone = board.call_stdin("plan", &with(WHOLE, &owe("/controls/x/gone")));
    assert_eq!(codes(&gone), ["changes::ref::dangling"], "{gone}");
    assert_eq!(stored(&board, &gone).get("owed"), None);
    let output = board.call_stdin("plan", &with(WHOLE, &owe("/controls/x/owed")));
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
    let output = board.call_stdin("plan", &with(WHOLE, &fields));
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
    let text = format!("{WHOLE}\n### Considered\n\n- /tasks/a: unrelated.\n");
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
    let owed = format!("{WHOLE}\n## Obligations left owed\n\n- /controls/x: later.\n");
    let warned = board.call_stdin("plan", &given(&owed, &[]));
    assert_eq!(
        warned["warnings"][0]["code"],
        "changes::plan::considered_in_text"
    );
    let quiet = board.call_stdin("plan", &given(WHOLE, &[]));
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
    let first = board.call_stdin("plan", &with(WHOLE, &considered));
    assert_eq!(first["unchanged"], false);
    let same = board.call_stdin("plan", &with(WHOLE, &considered));
    assert_eq!(same["unchanged"], true);
    std::thread::sleep(std::time::Duration::from_millis(2));
    let other = board.call_stdin("plan", &with(WHOLE, &owed));
    assert_eq!(other["unchanged"], false);
}

/// The development flow's checks read all three answers, so its plan gives
/// each; the refusal comes before anything is written.
#[test]
fn a_plan_that_leaves_out_an_answer_the_flow_reads_is_refused_and_one_that_gives_each_is_written() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    let mut partial = given(WHOLE, &[]);
    partial["answers"] = json!({"data": "stored"});
    let refused = board.refusal_stdin("plan", &partial);
    assert_eq!(refused["code"], "changes::plan::no_answers", "{refused}");
    assert_eq!(
        refused["fields"]["answers"],
        json!(["lifecycle (none or record)", "ux (none or mockups)"])
    );
    let mut bare = given(WHOLE, &[]);
    bare.as_object_mut().unwrap().remove("answers");
    let refused = board.refusal_stdin("plan", &bare);
    assert_eq!(refused["code"], "changes::plan::no_answers", "{refused}");
    assert!(!board.exists("changes/t/plans"));
    let mut whole = given(WHOLE, &[]);
    whole["answers"] = json!({"ux": "mockups", "data": "stored", "lifecycle": "record"});
    let output = board.call_stdin("plan", &whole);
    assert_eq!(
        stored(&board, &output)["answers"],
        json!({"ux": "mockups", "data": "stored", "lifecycle": "record"})
    );
}

#[test]
fn a_plan_whose_answers_alone_differ_is_written_and_the_same_answers_again_write_nothing() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    let first = board.call_stdin("plan", &given(WHOLE, &[]));
    let mut stored_data = given(WHOLE, &[]);
    stored_data["answers"]["data"] = json!("stored");
    let second = board.call_stdin("plan", &stored_data);
    assert_eq!(second["unchanged"], false);
    assert_ne!(first["path"], second["path"]);
    assert_eq!(stored(&board, &second)["answers"]["data"], "stored");
    // The first plan's record is left as it was written.
    assert_eq!(stored(&board, &first)["answers"]["data"], "none");
    let again = board.call_stdin("plan", &stored_data);
    assert_eq!(again["unchanged"], true);
    assert_eq!(again["path"], second["path"]);
}
