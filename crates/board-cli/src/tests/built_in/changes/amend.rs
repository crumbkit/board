//! `changes.plan-amend`, which writes a new plan version that only adds
//! the two test sections to the change's current plan, and an amendment
//! record that names the headings it filled.

use super::*;

const CHANGE: &str = "/changes/t";
const PROVES: &str = "What the tests prove";
const DOES_NOT_PROVE: &str = "What the tests do not prove";

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
fn an_amend_appends_each_test_section_and_keeps_every_other_field_and_byte_of_the_plan() {
    let board = Changes::new();
    let first = planned(&board, PLAN);
    let before = at(&board, &first["path"]);
    let output = board.call(
        "plan-amend",
        &json!({"path": CHANGE, "proves": "The suite stops.", "does_not_prove": "Retries."}),
    );
    let plan = &output["path"];
    assert_ne!(plan, &first["path"]);
    assert_eq!(board.read("changes/t.json")["plan"], *plan);
    assert_eq!(
        text_of(&board, plan),
        format!("{PLAN}\n## {PROVES}\n\nThe suite stops.\n\n## {DOES_NOT_PROVE}\n\nRetries.\n")
    );
    let mut expected = before.clone();
    expected["text"] = output["text"].clone();
    assert_eq!(at(&board, plan), expected);
    assert_eq!(
        at(&board, &output["amendment"]),
        json!({
            "$type": "changes/amendment/v1",
            "plan": plan,
            "from": first["path"],
            "sections": [PROVES, DOES_NOT_PROVE],
        })
    );
    assert_eq!(output["sections"], json!([PROVES, DOES_NOT_PROVE]));
    assert!(output.get("paths").is_none(), "{output}");
    assert!(output.get("owed").is_none(), "{output}");
    // The earlier plan is left as it was written.
    assert_eq!(at(&board, &first["path"]), before);
    assert_eq!(board.code(output["amendment"].as_str().unwrap()), None);
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
fn an_amend_with_no_test_section_is_refused_and_one_with_a_section_is_written() {
    let board = Changes::new();
    planned(&board, PLAN);
    let refused = board.refusal("plan-amend", &json!({"path": CHANGE}));
    assert_eq!(refused["code"], "changes::amend::nothing", "{refused}");
    assert_eq!(amendments(&board), 0);
    board.call(
        "plan-amend",
        &json!({"path": CHANGE, "proves": "It stops."}),
    );
    assert_eq!(amendments(&board), 1);
}

/// The owner's answer /projects/board/answers/20261126T1601Z-004a lets an
/// agent add only the test sections: a path or an owed entry still needs
/// the owner's plan resend, so `plan-amend` takes neither.
#[test]
fn an_amend_that_adds_a_path_or_an_owed_entry_is_refused_and_one_with_a_section_is_written() {
    let board = Changes::new();
    let first = planned(&board, PLAN);
    let refused = board.refusal(
        "plan-amend",
        &json!({"path": CHANGE, "paths": "/repos/x/extra.rs", "proves": "It stops."}),
    );
    assert_eq!(refused["code"], "board::cli::usage", "{refused}");
    assert_eq!(refused["fields"]["argument"], "--paths", "{refused}");
    let refused = board.refusal(
        "plan-amend",
        &json!({"path": CHANGE, "owed": "/rules/x/owed", "proves": "It stops."}),
    );
    assert_eq!(refused["code"], "board::cli::usage", "{refused}");
    assert_eq!(refused["fields"]["argument"], "--owed", "{refused}");
    assert_eq!(amendments(&board), 0);
    assert_eq!(board.read("changes/t.json")["plan"], first["path"]);
    let output = board.call(
        "plan-amend",
        &json!({"path": CHANGE, "proves": "It stops."}),
    );
    assert_eq!(at(&board, &output["path"])["paths"], json!(PATHS));
    assert!(at(&board, &output["path"]).get("owed").is_none());
    assert_eq!(amendments(&board), 1);
}

/// `plan-amend` takes every input as a flag, so Board never reads JSON on
/// stdin for it: a `paths` or `owed` key there is refused as
/// `board::plugin::bad_input` and nothing is written, and the same text
/// as flags without the key is written.
#[test]
fn an_amend_on_stdin_with_a_paths_or_owed_key_is_refused_as_bad_input_and_the_same_flags_without_it_are_written()
 {
    let board = Changes::new();
    let first = planned(&board, PLAN);
    for (key, value) in [
        ("paths", json!(["/repos/x/extra.rs"])),
        ("owed", json!(["/rules/x/owed"])),
    ] {
        let mut given = json!({"path": CHANGE, "proves": "It stops."});
        given[key] = value;
        let refused = board.refusal_stdin("plan-amend", &given);
        assert_eq!(
            refused["code"], "board::plugin::bad_input",
            "{key}: {refused}"
        );
        let hint = refused["recovery"]["text"].as_str().unwrap();
        assert!(
            hint.contains(
                "Board ignored stdin because changes.plan-amend takes every input as a flag"
            ),
            "{key}: {hint}"
        );
    }
    assert_eq!(amendments(&board), 0);
    assert_eq!(board.read("changes/t.json")["plan"], first["path"]);
    let output = board.call(
        "plan-amend",
        &json!({"path": CHANGE, "proves": "It stops."}),
    );
    assert_eq!(at(&board, &output["path"])["paths"], json!(PATHS));
    assert_eq!(amendments(&board), 1);
}

#[test]
fn an_amend_of_a_change_with_no_plan_is_refused_and_one_with_a_plan_is_written() {
    let board = Changes::new();
    board.change_at("changes/t.json", "build", &json!({}));
    let given = json!({"path": CHANGE, "proves": "It stops."});
    let refused = board.refusal("plan-amend", &given);
    assert_eq!(refused["code"], "changes::amend::no_plan", "{refused}");
    planned(&board, PLAN);
    board.call("plan-amend", &given);
    assert_eq!(amendments(&board), 1);
}

#[test]
fn an_amend_of_a_done_change_is_refused_and_one_of_a_change_in_build_is_written() {
    let board = Changes::new();
    planned(&board, PLAN);
    let plan = board.read("changes/t.json")["plan"].clone();
    board.change_at("changes/t.json", "done", &json!({"plan": plan}));
    let given = json!({"path": CHANGE, "proves": "It stops."});
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
    let proved = board.call(
        "plan-amend",
        &json!({"path": CHANGE, "proves": "It stops."}),
    );
    std::thread::sleep(std::time::Duration::from_millis(2));
    let unproved = board.call(
        "plan-amend",
        &json!({"path": CHANGE, "does_not_prove": "Retries."}),
    );
    let next = board.call("next", &json!({"path": CHANGE}));
    assert_eq!(
        next["amended"],
        json!([
            {"path": proved["amendment"], "from": first["path"], "sections": [PROVES]},
            {"path": unproved["amendment"], "from": proved["path"],
             "sections": [DOES_NOT_PROVE]},
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
fn the_pull_request_body_lists_the_headings_each_amendment_filled() {
    let board = Changes::new();
    planned(&board, PLAN);
    let output = board.call(
        "plan-amend",
        &json!({"path": CHANGE, "proves": "It stops.", "does_not_prove": "Retries."}),
    );
    board.call("pr", &json!({"path": CHANGE}));
    let body = board.text("changes/t/pr.md");
    assert!(
        body.contains(&format!(
            "## Plan amendments\n\nThese were added to the plan after it was written, with \
             `board changes plan-amend`:\n\n- `{}` filled the sections {PROVES}, \
             {DOES_NOT_PROVE}\n",
            output["amendment"].as_str().unwrap()
        )),
        "{body}"
    );
}
