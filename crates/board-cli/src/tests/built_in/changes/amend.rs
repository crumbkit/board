//! `changes.plan-amend`, which writes a new plan version that only adds
//! paths, owed entries and test sections to the change's current plan, and
//! an amendment record that names what it added.

use super::*;

const CHANGE: &str = "/changes/t";
const PROVES: &str = "What the tests prove";
const DOES_NOT_PROVE: &str = "What the tests do not prove";

/// An obligation at `/controls/x/owed`, as `controls.check` would list it.
fn owed_control(board: &Changes) {
    let owed = json!({
        "$type": "controls/control/v1", "kind": "obligation", "requirement": "Sweep the docs.",
        "purpose": "p", "paths": ["repos/x/**"],
    });
    board.write("controls/x/owed.json", &owed.to_string());
}

/// A change in build whose plan `changes.plan` wrote with `text`, one used
/// document, one considered record and the fixture paths: the plan's
/// output.
fn planned(board: &Changes, text: &str) -> Value {
    board.change_at("changes/t.json", "build", &json!({}));
    board.write("standards/errors.md", "# Errors\n");
    board.write("notes/there.md", "# There\n");
    board.call_stdin(
        "plan",
        &json!({
            "path": CHANGE, "text": text, "uses": ["/standards/errors.md"], "paths": PATHS,
            "considered": [{"records": ["/notes/there.md"], "why": "Other."}],
            "answers": answers(),
        }),
    )
}

/// The record at a logical `path`, read from the board.
fn at(board: &Changes, path: &Value) -> Value {
    board.read(&format!(
        "{}.json",
        path.as_str().unwrap().trim_start_matches('/')
    ))
}

/// The text of the plan record at `path`.
fn text_of(board: &Changes, plan: &Value) -> String {
    let text = at(board, plan)["text"].as_str().unwrap().to_owned();
    board.text(text.trim_start_matches('/'))
}

/// The amendment records the board holds for /changes/t.
fn amendments(board: &Changes) -> usize {
    std::fs::read_dir(board.root().join("changes/t/amendments")).map_or(0, Iterator::count)
}

#[test]
fn an_amend_adds_a_path_and_keeps_every_other_field_and_the_text_of_the_plan() {
    let board = Changes::new();
    let first = planned(&board, PLAN);
    let before = at(&board, &first["path"]);
    let output = board.call(
        "plan-amend",
        &json!({"path": CHANGE, "paths": "/repos/x/extra.rs"}),
    );
    let plan = &output["path"];
    assert_ne!(plan, &first["path"]);
    assert_eq!(board.read("changes/t.json")["plan"], *plan);
    let mut expected = before.clone();
    expected["text"] = output["text"].clone();
    expected["paths"] = json!(["/repos/x/**", "/repos/x/extra.rs"]);
    assert_eq!(at(&board, plan), expected);
    assert_eq!(text_of(&board, plan), PLAN);
    assert_eq!(
        at(&board, &output["amendment"]),
        json!({
            "$type": "changes/amendment/v1",
            "plan": plan,
            "from": first["path"],
            "paths": ["/repos/x/extra.rs"],
        })
    );
    assert_eq!(output["paths"], json!(["/repos/x/extra.rs"]));
    // The earlier plan is left as it was written.
    assert_eq!(at(&board, &first["path"]), before);
    assert_eq!(board.code(output["amendment"].as_str().unwrap()), None);
}

#[test]
fn an_amend_adds_an_owed_entry_after_those_the_plan_holds() {
    let board = Changes::new();
    owed_control(&board);
    let first = planned(&board, PLAN);
    let owed = json!([{"rule": "/controls/x/owed", "why": "The next step does it."}]);
    let output = board.call_stdin("plan-amend", &json!({"path": CHANGE, "owed": owed}));
    assert_eq!(at(&board, &output["path"])["owed"], owed);
    assert_eq!(at(&board, &output["path"])["paths"], json!(PATHS));
    assert_eq!(text_of(&board, &output["path"]), PLAN);
    assert_eq!(at(&board, &output["amendment"])["owed"], owed);
    assert_eq!(at(&board, &output["amendment"])["from"], first["path"]);
    assert_eq!(output["owed"], json!(["/controls/x/owed"]));
}

#[test]
fn an_amend_appends_each_test_section_the_plan_lacks_under_its_own_heading() {
    let board = Changes::new();
    planned(&board, PLAN);
    let output = board.call(
        "plan-amend",
        &json!({"path": CHANGE, "proves": "The suite stops.", "does_not_prove": "Retries."}),
    );
    assert_eq!(
        text_of(&board, &output["path"]),
        format!("{PLAN}\n## {PROVES}\n\nThe suite stops.\n\n## {DOES_NOT_PROVE}\n\nRetries.\n")
    );
    assert_eq!(
        at(&board, &output["amendment"])["sections"],
        json!([PROVES, DOES_NOT_PROVE])
    );
    assert_eq!(at(&board, &output["path"])["paths"], json!(PATHS));
}

#[test]
fn an_amend_fills_an_empty_test_heading_in_place_and_refuses_one_that_holds_text() {
    let board = Changes::new();
    let held =
        format!("{PLAN}\n## {PROVES}\n\nIt stops.\n\n## {DOES_NOT_PROVE}\n\n## Notes\n\nN.\n");
    planned(&board, &held);
    let refused = board.refusal(
        "plan-amend",
        &json!({"path": CHANGE, "proves": "More.", "does_not_prove": "Retries."}),
    );
    assert_eq!(refused["code"], "changes::amend::section_held", "{refused}");
    assert_eq!(refused["fields"]["sections"], json!([PROVES]));
    assert_eq!(amendments(&board), 0);
    let output = board.call(
        "plan-amend",
        &json!({"path": CHANGE, "does_not_prove": "Retries."}),
    );
    assert_eq!(
        text_of(&board, &output["path"]),
        format!(
            "{PLAN}\n## {PROVES}\n\nIt stops.\n\n## {DOES_NOT_PROVE}\n\nRetries.\n\n## Notes\n\nN.\n"
        )
    );
}

#[test]
fn an_amend_that_adds_nothing_new_is_refused_and_one_that_adds_a_new_path_is_written() {
    let board = Changes::new();
    planned(&board, PLAN);
    let refused = board.refusal("plan-amend", &json!({"path": CHANGE}));
    assert_eq!(refused["code"], "changes::amend::nothing", "{refused}");
    let refused = board.refusal(
        "plan-amend",
        &json!({"path": CHANGE, "paths": "/repos/x/**"}),
    );
    assert_eq!(refused["code"], "changes::amend::nothing", "{refused}");
    assert_eq!(amendments(&board), 0);
    let output = board.call_stdin(
        "plan-amend",
        &json!({"path": CHANGE, "paths": ["/repos/x/**", "/repos/x/new.rs"]}),
    );
    assert_eq!(output["paths"], json!(["/repos/x/new.rs"]));
}

#[test]
fn an_amend_of_a_change_with_no_plan_is_refused_and_one_with_a_plan_is_written() {
    let board = Changes::new();
    board.change_at("changes/t.json", "build", &json!({}));
    let given = json!({"path": CHANGE, "paths": "/repos/x/a.rs"});
    let refused = board.refusal("plan-amend", &given);
    assert_eq!(refused["code"], "changes::amend::no_plan", "{refused}");
    planned(&board, PLAN);
    board.call("plan-amend", &given);
    assert_eq!(amendments(&board), 1);
}

#[test]
fn an_amend_with_a_path_outside_the_mount_is_refused_and_one_inside_it_is_written() {
    let board = Changes::new();
    planned(&board, PLAN);
    let refused = board.refusal(
        "plan-amend",
        &json!({"path": CHANGE, "paths": "/repos/y/a.rs"}),
    );
    assert_eq!(refused["code"], "changes::plan::outside_mount", "{refused}");
    assert_eq!(amendments(&board), 0);
    board.call(
        "plan-amend",
        &json!({"path": CHANGE, "paths": "/repos/x/a.rs"}),
    );
    assert_eq!(amendments(&board), 1);
}

#[test]
fn an_amend_owing_a_path_that_holds_nothing_is_refused_and_one_owing_a_rule_is_written() {
    let board = Changes::new();
    owed_control(&board);
    planned(&board, PLAN);
    let owe = |rule: &str| json!({"path": CHANGE, "owed": [{"rule": rule, "why": "Later."}]});
    let refused = board.refusal_stdin("plan-amend", &owe("/controls/x/none"));
    assert_eq!(refused["code"], "changes::ref::dangling", "{refused}");
    assert_eq!(amendments(&board), 0);
    board.call_stdin("plan-amend", &owe("/controls/x/owed"));
    assert_eq!(amendments(&board), 1);
}

#[test]
fn an_amend_of_a_done_change_is_refused_and_one_of_a_change_in_build_is_written() {
    let board = Changes::new();
    planned(&board, PLAN);
    let plan = board.read("changes/t.json")["plan"].clone();
    board.change_at("changes/t.json", "done", &json!({"plan": plan}));
    let given = json!({"path": CHANGE, "paths": "/repos/x/a.rs"});
    let refused = board.refusal("plan-amend", &given);
    assert_eq!(refused["code"], "changes::phase::final", "{refused}");
    board.change_at("changes/t.json", "build", &json!({"plan": plan}));
    board.call("plan-amend", &given);
    assert_eq!(amendments(&board), 1);
}

#[test]
fn an_amend_that_takes_the_text_over_3000_words_is_refused_and_one_within_it_is_written() {
    let board = Changes::new();
    let long = format!("# Long\n{}\n", " w".repeat(2_990));
    planned(&board, &long);
    let refused = board.refusal(
        "plan-amend",
        &json!({"path": CHANGE, "proves": " w".repeat(10).trim()}),
    );
    assert_eq!(refused["code"], "changes::plan::too_long", "{refused}");
    assert_eq!(amendments(&board), 0);
    board.call(
        "plan-amend",
        &json!({"path": CHANGE, "proves": "It stops."}),
    );
    assert_eq!(amendments(&board), 1);
}

#[test]
fn next_lists_the_amendments_since_the_last_full_plan_and_none_after_a_new_one() {
    let board = Changes::new();
    let first = planned(&board, PLAN);
    let added = board.call(
        "plan-amend",
        &json!({"path": CHANGE, "paths": "/repos/x/a.rs"}),
    );
    std::thread::sleep(std::time::Duration::from_millis(2));
    let tested = board.call(
        "plan-amend",
        &json!({"path": CHANGE, "proves": "It stops."}),
    );
    let next = board.call("next", &json!({"path": CHANGE}));
    assert_eq!(
        next["amended"],
        json!([
            {"path": added["amendment"], "from": first["path"], "paths": ["/repos/x/a.rs"],
             "owed": [], "sections": []},
            {"path": tested["amendment"], "from": added["path"], "paths": [], "owed": [],
             "sections": [PROVES]},
        ])
    );
    std::thread::sleep(std::time::Duration::from_millis(2));
    board.call_stdin(
        "plan",
        &json!({"path": CHANGE, "text": PLAN, "paths": PATHS, "answers": answers()}),
    );
    let next = board.call("next", &json!({"path": CHANGE}));
    assert!(next.get("amended").is_none(), "{next}");
}

#[test]
fn the_pull_request_body_lists_what_each_amendment_added() {
    let board = Changes::new();
    owed_control(&board);
    planned(&board, PLAN);
    let output = board.call_stdin(
        "plan-amend",
        &json!({"path": CHANGE, "paths": ["/repos/x/a.rs"], "proves": "It stops.",
                "owed": [{"rule": "/controls/x/owed", "why": "Later."}]}),
    );
    board.call("pr", &json!({"path": CHANGE}));
    let body = board.text("changes/t/pr.md");
    assert!(
        body.contains(&format!(
            "## Plan amendments\n\nThese were added to the plan after it was written, with \
             `board changes plan-amend`:\n\n- `{}` added the paths `/repos/x/a.rs`; the owed \
             entries `/controls/x/owed`; the sections {PROVES}\n",
            output["amendment"].as_str().unwrap()
        )),
        "{body}"
    );
}
