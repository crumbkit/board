//! `board plugin list` and `find`.

use super::*;

const TASKS: &str = r#"{
  "$type": "board/plugin-manifest/v1",
  "name": "tasks",
  "version": "0.1.0",
  "description": "Open tasks.",
  "run": ["./run"],
  "metadata": {"area": "planning"},
  "capabilities": [{
    "name": "due",
    "description": "Open tasks due before a date.",
    "input_schema": {"type": "object"},
    "output_schema": {"type": "object"}
  }]
}"#;

/// A board enabling tasks, and a plugins directory holding it.
pub(super) fn installed() -> (Scenario, Scenario) {
    let board = Scenario::new();
    board.write(
        ".board/board.json",
        Item::File(r#"{"$type": "board/config/v1", "plugins": ["tasks"]}"#),
    );
    let data = Scenario::new();
    data.write("plugins/tasks/plugin.json", Item::File(TASKS));
    (board, data)
}

fn plugin(board: &Scenario, data: &Scenario, arguments: &[&str]) -> Output {
    let plugins = data.workdir().join("plugins");
    board_full(board.workdir(), None, Some(&plugins), arguments, b"")
}

#[test]
fn plugin_list_prints_each_plugin_with_its_capabilities() {
    let (board, data) = installed();
    let output = plugin(&board, &data, &["plugin", "list"]);
    assert_eq!(output.code, 0, "{}", text(&output.stderr));
    assert_eq!(
        text(&output.stdout),
        "ok tasks 0.1.0 enabled\n  due Open tasks due before a date.\n"
    );
}

#[test]
fn plugin_list_json_is_a_tagged_report_holding_the_manifest() {
    let (board, data) = installed();
    let report = json(&plugin(&board, &data, &["--json", "plugin", "list"]));
    assert_eq!(report["$type"], "board/plugin-list-report/v1");
    assert_eq!(report["degraded"], false);
    let tasks = &report["plugins"][0];
    assert_eq!(tasks["name"], "tasks");
    assert_eq!(tasks["status"], "ok");
    assert_eq!(tasks["enabled"], true);
    assert_eq!(tasks["run"], serde_json::json!(["./run"]));
    assert_eq!(tasks["capabilities"][0]["name"], "due");
    assert_eq!(
        tasks["capabilities"][0]["input_schema"],
        serde_json::json!({"type": "object"})
    );
}

#[test]
fn plugin_list_fails_when_a_plugin_is_broken_or_missing() {
    let (board, data) = installed();
    data.write("plugins/broken/plugin.json", Item::File("{}"));
    let output = plugin(&board, &data, &["--json", "plugin", "list"]);
    assert_eq!(output.code, 1);
    let report = json(&output);
    assert_eq!(report["degraded"], true);
    assert_eq!(report["plugins"][0]["code"], "board::plugin::bad_manifest");
    assert_eq!(report["plugins"][1]["status"], "ok");

    data.write("plugins/broken", Item::Deleted)
        .write("plugins/tasks", Item::Deleted);
    let output = plugin(&board, &data, &["plugin", "list"]);
    assert_eq!(output.code, 1);
    assert_eq!(
        text(&output.stdout),
        "degraded board::plugin::not_installed tasks enabled\n"
    );
}

#[test]
fn plugin_find_prints_one_line_per_matching_capability() {
    let (board, data) = installed();
    let output = plugin(
        &board,
        &data,
        &[
            "plugin",
            "find",
            "--capability",
            "due",
            "--meta",
            "area=planning",
        ],
    );
    assert_eq!(output.code, 0);
    assert_eq!(
        text(&output.stdout),
        "tasks.due enabled Open tasks due before a date.\n"
    );
    let output = plugin(&board, &data, &["plugin", "find", "--meta", "area=writing"]);
    assert_eq!((output.code, text(&output.stdout)), (0, String::new()));
}

#[test]
fn plugin_find_json_names_the_plugin_and_capability() {
    let (board, data) = installed();
    let report = json(&plugin(
        &board,
        &data,
        &["--json", "plugin", "find", "--capability", "due"],
    ));
    assert_eq!(report["$type"], "board/plugin-find-report/v1");
    let found = &report["capabilities"][0];
    assert_eq!(
        (&found["plugin"], &found["capability"], &found["version"]),
        (&"tasks".into(), &"due".into(), &"0.1.0".into())
    );
}

#[test]
fn a_capability_or_filter_that_cannot_be_read_refuses_the_command_line() {
    let (board, data) = installed();
    for refused in [
        &["plugin", "find", "--capability", "Due"][..],
        &["plugin", "find", "--meta", "area"],
        &["plugin", "find"],
    ] {
        assert_eq!(plugin(&board, &data, refused).code, 2, "{refused:?}");
    }
    assert_eq!(
        plugin(&board, &data, &["plugin", "find", "--capability", "due"]).code,
        0
    );
    assert_eq!(
        plugin(&board, &data, &["plugin", "find", "--meta", "area=x"]).code,
        0
    );
}
