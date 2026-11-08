//! Which flow a change follows: `start` chooses one and keeps a copy, the
//! gates read the copy, and `reflow` moves a change to another flow.

use super::*;

const TEAM: &str = include_str!("../../../../../board-plugin-changes/src/flow/flows-md/team.json");
const DEVELOPMENT: [&str; 4] = ["plan", "build", "review", "land"];

/// A flow called `name` with `phases`, each with no checks.
fn flow(name: &str, phases: &[&str]) -> String {
    let listed: Vec<Value> = (phases.iter())
        .map(|phase| {
            json!({"name": phase, "agent": "lead", "instruction": format!("Do {phase}."), "checks": []})
        })
        .collect();
    json!({
        "$type": "changes/flow/v1",
        "name": name,
        "summary": "A flow for a test.",
        "stop": {"through": phases[0]},
        "phases": listed,
    })
    .to_string()
}

/// A link to `path`.
fn link(path: &str) -> String {
    json!({"$type": "board/mount/v1", "path": path}).to_string()
}

/// The fixture board with /repos/x mounted.
fn mounted() -> Changes {
    let board = Changes::new();
    board.mount_x(&[("README.md", "x\n")]);
    board
}

/// The input of `start` for a change titled `title` on /repos/x, with
/// `fields`.
fn starting(title: &str, fields: &Value) -> Value {
    let input = json!({"mount": "/repos/x", "task": TASK, "title": title});
    serde_json::from_str(&record(input, fields)).unwrap()
}

/// The name and phases of the flow the change `start` wrote follows.
fn started(board: &Changes, title: &str, fields: &Value) -> (Value, Value) {
    let output = board.call("start", &starting(title, fields));
    (
        output["flow"]["name"].clone(),
        output["flow"]["phases"].clone(),
    )
}

#[test]
fn start_follows_the_development_flow_unless_the_board_holds_its_own_flow_of_that_name() {
    let board = mounted();
    assert_eq!(
        started(&board, "A", &json!({})),
        (json!("development"), json!(DEVELOPMENT))
    );
    board.write(
        "flows/development.json",
        &flow("development", &["draft", "land"]),
    );
    let output = board.call("start", &starting("B", &json!({})));
    assert_eq!(
        (&output["flow"]["phases"], &output["phase"]),
        (&json!(["draft", "land"]), &json!("draft"))
    );
    assert_eq!(board.read("changes/b.json")["phase"], "draft");
    assert_eq!(
        board.read("changes/b/flow.json")["phases"][0]["name"],
        "draft"
    );
}

#[test]
fn the_repositorys_link_wins_over_the_default_link_and_start_flow_wins_over_both() {
    let board = mounted();
    board.write("flows/quick.json", &flow("quick", &["build", "land"]));
    board.write("flows/mine.json", &flow("mine", &["draft", "land"]));
    board.write("flows/default.json", &link("/flows/quick"));
    assert_eq!(started(&board, "A", &json!({})).0, "quick");
    assert_eq!(board.read("changes/a.json")["phase"], "build");
    board.write("flows/repos/x.json", &link("/flows/mine"));
    assert_eq!(started(&board, "B", &json!({})).0, "mine");
    assert_eq!(started(&board, "C", &json!({"flow": "small"})).0, "small");
}

#[test]
fn start_refuses_a_flow_neither_the_board_holds_nor_the_plugin_ships_and_admits_a_shipped_one() {
    let board = mounted();
    let refused = board.refusal("start", &starting("A", &json!({"flow": "nope"})));
    assert_eq!(
        (&refused["code"], &refused["fields"]["flow"]),
        (&json!("changes::flow::not_found"), &json!("nope"))
    );
    assert!(!board.exists("changes/a.json"));
    assert_eq!(started(&board, "A", &json!({"flow": "small"})).0, "small");
}

#[test]
fn a_default_link_to_a_flow_the_board_does_not_hold_is_refused_and_one_to_a_flow_it_holds_admitted()
{
    let board = mounted();
    board.write("flows/default.json", &link("/flows/quick"));
    let refused = board.refusal("start", &starting("A", &json!({})));
    assert_eq!(
        (&refused["code"], &refused["fields"]["path"]),
        (&json!("changes::flow::not_found"), &json!("/flows/default"))
    );
    board.write("flows/quick.json", &flow("quick", &["build", "land"]));
    assert_eq!(started(&board, "A", &json!({})).0, "quick");
}

/// The ids of the checks `next` lists for /changes/a.
fn gate(board: &Changes) -> Vec<String> {
    let output = board.call("next", &json!({"path": "/changes/a"}));
    (output["gate"].as_array().unwrap().iter())
        .map(|check| check["check"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn the_gate_reads_the_changes_copy_so_editing_the_boards_flow_moves_only_a_reflowed_change() {
    let board = mounted();
    board.write("flows/mine.json", &flow("mine", &["draft", "land"]));
    started(&board, "A", &json!({"flow": "mine"}));
    let mut edited: Value = serde_json::from_str(&flow("mine", &["draft", "land"])).unwrap();
    edited["phases"][0]["checks"] = json!([{"id": "head", "kind": "field", "field": "head"}]);
    board.write("flows/mine.json", &edited.to_string());
    assert_eq!(gate(&board), Vec::<String>::new());
    let reflowed = board.call("reflow", &json!({"path": "/changes/a", "flow": "mine"}));
    assert_eq!(
        reflowed,
        json!({"path": "/changes/a", "flow": {"name": "mine", "phases": ["draft", "land"]}, "phase": "draft"})
    );
    assert_eq!(gate(&board), ["head"]);
}

#[test]
fn reflow_refuses_a_flow_without_the_changes_phase_and_admits_one_with_it() {
    let board = mounted();
    started(&board, "A", &json!({}));
    let refused = board.refusal("reflow", &json!({"path": "/changes/a", "flow": "small"}));
    assert_eq!(
        (
            &refused["code"],
            &refused["fields"]["phase"],
            &refused["fields"]["phases"]
        ),
        (
            &json!("changes::reflow::no_phase"),
            &json!("plan"),
            &json!(["build", "review", "land"])
        )
    );
    assert_eq!(board.read("changes/a/flow.json")["name"], "development");
    board.write("flows/small.json", &flow("small", &["plan", "land"]));
    let moved = board.call("reflow", &json!({"path": "/changes/a", "flow": "small"}));
    assert_eq!(moved["flow"]["name"], "small");
    assert_eq!(
        board.read("changes/a/flow.json")["phases"][1]["name"],
        "land"
    );
}

#[test]
fn reflow_refuses_a_flow_that_is_not_there_and_admits_a_shipped_one() {
    let board = mounted();
    started(&board, "A", &json!({}));
    let refused = board.refusal("reflow", &json!({"path": "/changes/a", "flow": "nope"}));
    assert_eq!(refused["code"], "changes::flow::not_found");
    let moved = board.call(
        "reflow",
        &json!({"path": "/changes/a", "flow": "development"}),
    );
    assert_eq!(moved["phase"], "plan");
}

#[test]
fn reflow_and_attach_suite_refuse_a_v1_change_until_it_is_migrated() {
    let board = mounted();
    board.change_at("changes/a.json", "plan", &json!({}));
    let refused = board.refusal(
        "reflow",
        &json!({"path": "/changes/a", "flow": "development"}),
    );
    assert_eq!(refused["code"], "changes::change::not_migrated");
    board.write("suites/checks.json", &typed("runs/suite/v1", &json!({})));
    let suite = json!({"path": "/changes/a", "suite": "/suites/checks"});
    assert_eq!(
        board.refusal("attach", &suite)["code"],
        "changes::change::not_migrated"
    );
    board.call("migrate", &json!({"apply": true}));
    board.call(
        "reflow",
        &json!({"path": "/changes/a", "flow": "development"}),
    );
    board.call("attach", &suite);
    assert_eq!(board.read("changes/a.json")["suite"], "/suites/checks");
}

#[test]
fn attach_suite_refuses_a_record_that_is_no_suite_and_sets_one_that_is() {
    let board = mounted();
    started(&board, "A", &json!({}));
    board.write(
        "notes/a.json",
        &typed("plans/decision/v1", &json!({"refs": []})),
    );
    let wrong = json!({"path": "/changes/a", "suite": "/notes/a"});
    assert_eq!(
        board.refusal("attach", &wrong)["code"],
        "changes::ref::wrong_type"
    );
    board.write("suites/checks.json", &typed("runs/suite/v1", &json!({})));
    board.call(
        "attach",
        &json!({"path": "/changes/a", "suite": "/suites/checks"}),
    );
    assert_eq!(board.read("changes/a.json")["suite"], "/suites/checks");
}

#[test]
fn a_change_whose_flow_copy_is_gone_is_refused_as_unreadable_and_reflow_restores_it() {
    let board = mounted();
    started(&board, "A", &json!({}));
    board.delete("changes/a/flow.json");
    let refused = board.refusal("next", &json!({"path": "/changes/a"}));
    assert_eq!(refused["code"], "changes::flow::unreadable");
    board.call(
        "reflow",
        &json!({"path": "/changes/a", "flow": "development"}),
    );
    assert_eq!(
        board.call("next", &json!({"path": "/changes/a"}))["phase"],
        "plan"
    );
}

#[test]
fn the_team_flow_leaves_plan_once_the_change_attaches_a_jira_issue_and_not_before() {
    let board = mounted();
    board.write("flows/team.json", TEAM);
    let output = board.call("start", &starting("A", &json!({"flow": "team"})));
    let text = output["plan"].as_str().unwrap().trim_start_matches('/');
    board.write(text, "# A\n\n## Problem\n\nX.\n\n## Outcome\n\nY.\n");
    let path = json!({"path": "/changes/a"});
    let refused = board.refusal("advance", &path);
    assert_eq!(
        (&refused["code"], &refused["fields"]["missing"]),
        (&json!("changes::gate::unmet"), &json!(["jira-ticket"]))
    );
    board.write(
        "gh/o/r/issues/2.json",
        &typed(
            "github/issue/v1",
            &json!({"owner": "o", "repo": "r", "number": 2}),
        ),
    );
    board.call(
        "attach",
        &json!({"path": "/changes/a", "ticket": "/gh/o/r/issues/2"}),
    );
    assert_eq!(
        board.refusal("advance", &path)["fields"]["missing"],
        json!(["jira-ticket"])
    );
    board.write(
        "jira/T-1.json",
        &typed("jira/issue/v1", &json!({"key": "T-1"})),
    );
    board.call(
        "attach",
        &json!({"path": "/changes/a", "ticket": "/jira/T-1"}),
    );
    assert_eq!(board.call("advance", &path)["to"], "build");
}

#[test]
fn start_stores_the_suite_and_surfaces_the_open_records_on_the_mount_and_its_obligations() {
    let board = mounted();
    board.write("suites/checks.json", &typed("runs/suite/v1", &json!({})));
    board.write("tasks/open.md", &task("open"));
    board.write("tasks/done.md", &task("done"));
    board.write(
        "projects/x/decisions/d.json",
        &typed("plans/decision/v1", &json!({"refs": []})),
    );
    board.write(
        ".board/types/evals/eval/v1.schema.json",
        r#"{"type": "object"}"#,
    );
    board.write(
        "evals/e.json",
        &typed("evals/eval/v1", &json!({"mounts": ["/repos/x"]})),
    );
    board.write(
        "evals/other.json",
        &typed("evals/eval/v1", &json!({"mounts": ["/repos/y"]})),
    );
    let owed = json!({
        "$type": "controls/control/v1",
        "kind": "obligation",
        "requirement": "Sweep the docs.",
        "purpose": "p",
        "paths": ["repos/x/**"],
    });
    board.write("controls/x/docs.json", &owed.to_string());
    let output = board.call("start", &starting("A", &json!({"suite": "/suites/checks"})));
    assert_eq!(output["suite"], "/suites/checks");
    let surfaced = &output["surfaced"];
    assert_eq!(
        (
            &surfaced["tasks"]["paths"],
            &surfaced["decisions"]["paths"],
            &surfaced["evals"]["paths"]
        ),
        (
            &json!(["/tasks/open.md"]),
            &json!(["/projects/x/decisions/d"]),
            &json!(["/evals/e"])
        )
    );
    assert_eq!(
        output["obligations"],
        json!([{"control": "/controls/x/docs", "summary": "Sweep the docs."}])
    );
    assert_eq!(output["obligations_left_out"], 0);
    let change = board.read("changes/a.json");
    assert_eq!(
        (&change["suite"], &change["surfaced"]),
        (
            &json!("/suites/checks"),
            &json!(["/tasks/open.md", "/projects/x/decisions/d", "/evals/e"])
        )
    );
    let none = board.call("start", &starting("B", &json!({})));
    assert_eq!(none["suite"], Value::Null);
}

#[test]
fn start_refuses_a_suite_that_is_not_there_and_admits_one_that_is() {
    let board = mounted();
    let refused = board.refusal("start", &starting("A", &json!({"suite": "/suites/checks"})));
    assert_eq!(refused["code"], "changes::ref::dangling");
    board.write("suites/checks.json", &typed("runs/suite/v1", &json!({})));
    assert_eq!(
        board.call("start", &starting("A", &json!({"suite": "/suites/checks"})))["suite"],
        "/suites/checks"
    );
}

#[test]
fn next_names_the_owner_and_what_to_ask_when_only_the_owner_can_pass_the_checks_left() {
    let board = mounted();
    started(&board, "A", &json!({"handle": "@luke"}));
    board.call("attach", &json!({"path": "/changes/a", "route": "owner"}));
    let text = board.read("changes/a.json")["plan"]
        .as_str()
        .unwrap()
        .to_owned();
    board.write(&format!("{}.md", text.trim_start_matches('/')), PLAN);
    let output = board.call("next", &json!({"path": "/changes/a"}));
    assert_eq!(output["waiting_on"], json!(["owner"]));
    let ask = output["ask"].as_str().unwrap();
    assert!(ask.contains("- owner-approval: "), "{ask}");
    assert!(output["instruction"].as_str().unwrap().contains("`ask`"));
    board.write(&format!("{}.md", text.trim_start_matches('/')), "# A\n");
    let output = board.call("next", &json!({"path": "/changes/a"}));
    assert_eq!(
        (&output["waiting_on"], &output["ask"]),
        (&Value::Null, &Value::Null)
    );
}

#[test]
fn plan_takes_a_plan_with_no_uses_as_one_that_uses_nothing() {
    let board = mounted();
    started(&board, "A", &json!({}));
    let written = board.call_stdin(
        "plan",
        &json!({"path": "/changes/a", "text": PLAN, "paths": PATHS}),
    );
    let plan = written["path"]
        .as_str()
        .unwrap()
        .trim_start_matches('/')
        .to_owned();
    assert_eq!(board.read(&format!("{plan}.json"))["uses"], json!([]));
}

#[test]
fn start_and_reflow_refuse_a_board_flow_with_a_phase_named_twice_or_done_and_admit_its_twin() {
    let board = mounted();
    for (phases, code) in [
        (&["build", "build"][..], "changes::flow::duplicate_phase"),
        (&["build", "done"][..], "changes::flow::reserved_phase"),
    ] {
        board.write("flows/mine.json", &flow("mine", phases));
        let refused = board.refusal("start", &starting("A", &json!({"flow": "mine"})));
        assert_eq!(
            (&refused["code"], &refused["fields"]["codes"]),
            (&json!("changes::flow::invalid"), &json!([code]))
        );
    }
    board.write("flows/mine.json", &flow("mine", &["build", "land"]));
    started(&board, "A", &json!({"flow": "mine"}));
    board.write("flows/other.json", &flow("other", &["build", "build"]));
    let refused = board.refusal("reflow", &json!({"path": "/changes/a", "flow": "other"}));
    assert_eq!(
        refused["fields"]["codes"],
        json!(["changes::flow::duplicate_phase"])
    );
    board.write("flows/other.json", &flow("other", &["build", "review"]));
    let moved = board.call("reflow", &json!({"path": "/changes/a", "flow": "other"}));
    assert_eq!(moved["flow"]["name"], "other");
}

#[test]
fn start_lists_ten_obligations_on_the_mount_and_counts_the_rest() {
    let board = mounted();
    let owe = |count: usize| {
        for at in 0..count {
            let owed = json!({
                "$type": "controls/control/v1",
                "kind": "obligation",
                "requirement": "Sweep the docs.",
                "purpose": "p",
                "paths": ["repos/x/**"],
            });
            board.write(&format!("controls/x/o{at:02}.json"), &owed.to_string());
        }
    };
    owe(10);
    let ten = board.call("start", &starting("A", &json!({})));
    assert_eq!(
        (
            ten["obligations"].as_array().unwrap().len(),
            &ten["obligations_left_out"]
        ),
        (10, &json!(0))
    );
    owe(12);
    let twelve = board.call("start", &starting("B", &json!({})));
    assert_eq!(
        (
            twelve["obligations"].as_array().unwrap().len(),
            &twelve["obligations_left_out"]
        ),
        (10, &json!(2))
    );
}

#[test]
fn next_on_a_small_change_in_build_names_the_plan_input() {
    let board = mounted();
    assert_eq!(started(&board, "A", &json!({"flow": "small"})).0, "small");
    let said = board.call("next", &json!({"path": "/changes/a"}))["instruction"].clone();
    assert!(said.as_str().unwrap().contains(NEW_PLAN_USAGE), "{said}");
}
