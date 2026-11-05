//! `test_prototype.py`: `changes.prototype` and `changes.verdict`, for a
//! change that answers one question on its own branch and never lands.

use super::*;

const CHANGE: &str = "/changes/step-list";
const QUESTION: &str = "Does a step list read better than a graph?";
const PARENT: &str = "/tasks/flows-editor.md";

/// A prototype change in `phase`, still exploring its question, with
/// `fields`.
fn prototype(phase: &str, fields: &Value) -> String {
    let given = json!({"kind": "prototype", "question": QUESTION, "state": "exploring"});
    change(
        phase,
        &serde_json::from_str(&record(given, fields)).unwrap(),
    )
}

fn exploring() -> String {
    prototype("build", &json!({}))
}

/// `given` with `fields`.
fn with(given: &Value, fields: &Value) -> Value {
    serde_json::from_str(&record(given.clone(), fields)).unwrap()
}

/// The tasks board with a repository mounted at /repos/x and the parent
/// task: the input of `changes.prototype`.
fn proposing(board: &Changes) -> Value {
    let mount = board.mount_x(&[("README.md", "x")]);
    board.write(PARENT.trim_start_matches('/'), &task("open"));
    json!({"mount": mount, "question": QUESTION, "parent": PARENT, "title": "Step list"})
}

/// The paths of the changes in an output of `changes.open`.
fn paths(output: &Value) -> Vec<String> {
    (output["changes"].as_array().unwrap().iter())
        .map(|entry| entry["path"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn prototype_adds_its_task_under_the_parent_and_writes_an_exploring_change_in_build() {
    let board = Changes::with_tasks();
    let given = proposing(&board);
    let output = board.call("prototype", &given);
    assert_eq!(
        (&output["path"], &output["task"], &output["branch"]),
        (
            &json!(CHANGE),
            &json!("/tasks/step-list.md"),
            &json!("proto/step-list")
        )
    );
    assert_eq!(
        output["brief"]["brief_line"],
        format!("board-brief: {CHANGE}")
    );
    assert_eq!(output["brief"]["phase"], "build");
    let written = board.read("changes/step-list.json");
    let kept: BTreeMap<&str, &Value> = ["phase", "kind", "task", "question", "state"]
        .into_iter()
        .map(|key| (key, &written[key]))
        .collect();
    assert_eq!(
        kept,
        BTreeMap::from([
            ("phase", &json!("build")),
            ("kind", &json!("prototype")),
            ("task", &json!("/tasks/step-list.md")),
            ("question", &json!(QUESTION)),
            ("state", &json!("exploring")),
        ])
    );
    let task = board.text("tasks/step-list.md");
    assert!(
        task.contains(r#"parent: "/tasks/flows-editor.md""#),
        "{task}"
    );
    let plan = board.text(written["plan"].as_str().unwrap().trim_start_matches('/'));
    assert_eq!(plan, format!("# Step list\n\n## Question\n\n{QUESTION}\n"));
}

#[test]
fn prototype_refuses_a_name_a_change_already_has_and_admits_another() {
    let board = Changes::with_tasks();
    let given = proposing(&board);
    board.write("changes/step-list.json", &exploring());
    assert_eq!(
        board.refusal("prototype", &given)["code"],
        "changes::change::exists"
    );
    let output = board.call("prototype", &with(&given, &json!({"name": "step-list-2"})));
    assert_eq!(output["path"], "/changes/step-list-2");
}

#[test]
fn a_prototype_refuses_to_advance_and_a_build_change_in_build_is_held_by_its_own_gate() {
    let board = Changes::new();
    board.write("changes/step-list.json", &exploring());
    let refused = board.refusal("advance", &json!({"path": CHANGE}));
    assert_eq!(
        (&refused["code"], &refused["fields"]["missing"]),
        (
            &json!("changes::prototype::never_lands"),
            &json!(["verdict"])
        )
    );
    board.write("changes/step-list.json", &change("build", &json!({})));
    assert_eq!(
        board.refusal("advance", &json!({"path": CHANGE}))["code"],
        "changes::gate::unmet"
    );
}

#[test]
fn next_tells_a_prototypes_builder_to_iterate_on_its_branch_and_ask_for_the_verdict() {
    let board = Changes::new();
    board.write("changes/step-list.json", &exploring());
    let output = board.call("next", &json!({"path": CHANGE}));
    let instruction = output["instruction"].as_str().unwrap();
    assert!(
        instruction.contains("on branch proto/step-list"),
        "{instruction}"
    );
    assert!(instruction.contains("changes.verdict"), "{instruction}");
}

#[test]
fn a_settled_verdict_ends_the_prototype_in_done_with_the_artifact_it_means() {
    let board = Changes::with_tasks();
    board.write("changes/step-list.json", &exploring());
    let artifact = "https://claude.ai/artifact/abc";
    let given = json!({
        "path": CHANGE,
        "state": "settled",
        "verdict": "A step list reads better.",
        "artifact": artifact,
    });
    assert_eq!(
        board.call("verdict", &given),
        json!({"path": CHANGE, "state": "settled", "phase": "done"})
    );
    let written = board.read("changes/step-list.json");
    assert_eq!(
        (
            &written["state"],
            &written["verdict"],
            &written["verdict_artifact"]
        ),
        (
            &json!("settled"),
            &json!("A step list reads better."),
            &json!(artifact)
        )
    );
    let task = board.text("tasks/given.md");
    assert!(task.contains(r#"status: "done""#), "{task}");
}

#[test]
fn an_abandoned_verdict_ends_the_prototype_in_dropped() {
    let board = Changes::with_tasks();
    board.write("changes/step-list.json", &exploring());
    let given =
        json!({"path": CHANGE, "state": "abandoned", "verdict": "The owner lost interest."});
    assert_eq!(board.call("verdict", &given)["phase"], "dropped");
    let written = board.read("changes/step-list.json");
    assert!(written.get("verdict_artifact").is_none(), "{written}");
    let task = board.text("tasks/given.md");
    assert!(task.contains(r#"status: "dropped""#), "{task}");
}

#[test]
fn verdict_refuses_a_build_change_and_admits_a_prototype() {
    let board = Changes::with_tasks();
    let given = json!({"path": CHANGE, "state": "settled", "verdict": "Yes."});
    board.write("changes/step-list.json", &change("build", &json!({})));
    assert_eq!(
        board.refusal("verdict", &given)["code"],
        "changes::prototype::not_prototype"
    );
    board.write("changes/step-list.json", &exploring());
    assert_eq!(board.call("verdict", &given)["phase"], "done");
}

#[test]
fn verdict_refuses_a_prototype_already_ended_and_admits_one_exploring() {
    let board = Changes::with_tasks();
    let given = json!({"path": CHANGE, "state": "settled", "verdict": "Yes."});
    board.write(
        "changes/step-list.json",
        &prototype("done", &json!({"state": "settled", "verdict": "Yes."})),
    );
    assert_eq!(
        board.refusal("verdict", &given)["code"],
        "changes::phase::final"
    );
    board.write("changes/step-list.json", &exploring());
    assert_eq!(board.call("verdict", &given)["phase"], "done");
}

#[test]
fn open_by_kind_prototype_lists_every_prototype_with_its_question_verdict_and_branch() {
    let board = Changes::new();
    board.write("changes/a.json", &change("build", &json!({})));
    board.write("changes/b.json", &exploring());
    board.write(
        "changes/c.json",
        &prototype("done", &json!({"state": "settled", "verdict": "Yes."})),
    );
    let output = board.call("open", &json!({"kind": "prototype"}));
    let listed: Vec<(&Value, &Value)> = (output["changes"].as_array().unwrap().iter())
        .map(|entry| (&entry["path"], &entry["kind"]))
        .collect();
    assert_eq!(
        listed,
        [
            (&json!("/changes/b"), &json!("prototype")),
            (&json!("/changes/c"), &json!("prototype")),
        ]
    );
    assert_eq!(
        output["changes"][1]["prototype"],
        json!({
            "question": QUESTION,
            "state": "settled",
            "verdict": "Yes.",
            "verdict_artifact": null,
            "branch": "proto/c",
        })
    );
    assert_eq!(
        paths(&board.call("open", &json!({"kind": "build"}))),
        ["/changes/a"]
    );
    assert_eq!(
        paths(&board.call("open", &json!({}))),
        ["/changes/a", "/changes/b"]
    );
}

#[test]
fn the_schema_refuses_an_unknown_kind_or_state_and_admits_a_prototype() {
    let board = Changes::new();
    board.write("repos/x/README.md", "x");
    board.write(
        "changes/kind.json",
        &prototype("build", &json!({"kind": "spike"})),
    );
    board.write(
        "changes/state.json",
        &prototype("build", &json!({"state": "paused"})),
    );
    board.write("changes/good.json", &exploring());
    assert_eq!(
        board.checked("/changes/"),
        codes(&[
            ("/changes/kind", Some("board::record::invalid")),
            ("/changes/state", Some("board::record::invalid")),
            ("/changes/good", None),
        ])
    );
}

#[test]
fn prototype_refuses_when_three_prototypes_explore_and_starts_once_one_is_settled() {
    let board = Changes::with_tasks();
    let given = proposing(&board);
    for number in 0..3 {
        board.write(&format!("changes/p{number}.json"), &exploring());
    }
    let refused = board.refusal("prototype", &given);
    assert_eq!(refused["code"], "changes::wip::full");
    let mount = given["mount"].as_str().unwrap();
    assert_eq!(
        refused["fields"]["override"],
        format!(
            "board call changes.prototype --mount {mount} --question '{QUESTION}' --parent {PARENT} --title 'Step list' --over_limit '<why>'"
        )
    );
    board.call(
        "verdict",
        &json!({"path": "/changes/p0", "state": "settled", "verdict": "Yes."}),
    );
    assert_eq!(board.call("prototype", &given)["path"], CHANGE);
}

#[test]
fn prototype_past_the_limit_with_a_reason_starts_and_warns() {
    let board = Changes::with_tasks();
    let given = proposing(&board);
    for number in 0..3 {
        board.write(&format!("changes/p{number}.json"), &exploring());
    }
    let output = board.call(
        "prototype",
        &with(&given, &json!({"over_limit": "Urgent."})),
    );
    assert_eq!(
        (&output["path"], &output["warning"]["code"]),
        (&json!(CHANGE), &json!("changes::wip::over_limit"))
    );
    assert_eq!(
        board.read("changes/step-list.json")["over_limit"],
        "Urgent."
    );
}

#[test]
fn a_verdict_leaves_a_task_already_closed_as_it_is() {
    let board = Changes::with_tasks();
    board.write("tasks/given.md", &task("done"));
    board.write("changes/step-list.json", &exploring());
    let given = json!({"path": CHANGE, "state": "abandoned", "verdict": "Not needed."});
    assert_eq!(board.call("verdict", &given)["phase"], "dropped");
    let task = board.text("tasks/given.md");
    assert!(task.contains("status: done"), "{task}");
}

#[test]
fn prototype_refuses_a_parent_outside_a_tasks_directory_and_admits_one_inside() {
    let board = Changes::with_tasks();
    let mount = board.mount_x(&[("README.md", "x")]);
    board.write("projects/flows.md", &task("open"));
    board.write(PARENT.trim_start_matches('/'), &task("open"));
    let given = json!({"mount": mount, "question": QUESTION, "title": "Step list"});
    let refused = board.refusal(
        "prototype",
        &with(&given, &json!({"parent": "/projects/flows.md"})),
    );
    assert_eq!(refused["code"], "changes::task::not_a_task_path");
    assert!(!board.exists("changes/step-list.json"));
    let output = board.call("prototype", &with(&given, &json!({"parent": PARENT})));
    assert_eq!(output["path"], CHANGE);
}

#[test]
fn build_changes_in_build_do_not_fill_the_prototype_cap() {
    let board = Changes::with_tasks();
    let given = proposing(&board);
    for number in 0..3 {
        board.write(
            &format!("changes/b{number}.json"),
            &change("build", &json!({})),
        );
    }
    assert_eq!(board.call("prototype", &given)["path"], CHANGE);
}

#[test]
fn a_paused_prototype_resumes_only_while_fewer_than_three_others_explore() {
    let board = Changes::new();
    board.write("changes/step-list.json", &exploring());
    assert_eq!(
        board.call("pause", &json!({"path": CHANGE}))["to"],
        "paused"
    );
    for number in 0..3 {
        board.write(&format!("changes/p{number}.json"), &exploring());
    }
    let refused = board.refusal("resume", &json!({"path": CHANGE}));
    assert_eq!(
        (&refused["code"], &refused["fields"]["kind"]),
        (&json!("changes::wip::full"), &json!("prototype"))
    );
    board.delete("changes/p0.json");
    assert_eq!(
        board.call("resume", &json!({"path": CHANGE}))["to"],
        "build"
    );
}

#[test]
fn a_verdict_ends_a_paused_prototype() {
    let board = Changes::with_tasks();
    board.write("changes/step-list.json", &prototype("paused", &json!({})));
    let given =
        json!({"path": CHANGE, "state": "abandoned", "verdict": "The owner lost interest."});
    assert_eq!(board.call("verdict", &given)["phase"], "dropped");
}
