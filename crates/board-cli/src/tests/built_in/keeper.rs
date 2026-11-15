//! The keeper plugin through the binary, with changes and tasks enabled
//! beside it. These tests answer the questions that the plugin's design
//! rests on: it owns its namespace, it reaches other plugins' capabilities
//! through the host, and it writes a file.

use super::*;

/// A board that enables keeper, changes and tasks.
fn board() -> Fixture {
    Fixture::enabling(&["keeper", "changes", "tasks"])
}

/// A `changes/change/v2` record with no mount, touched on 2026-11-10.
fn change(title: &str, phase: &str) -> String {
    json!({"$type": "changes/change/v2", "title": title, "phase": phase, "touched": "2026-11-10"})
        .to_string()
}

#[test]
fn a_plugin_may_own_the_namespace_keeper() {
    let board = board();
    let listed = board.json(&["plugin", "list"]);
    let names: Vec<&str> = (listed["data"].as_array().unwrap().iter())
        .filter_map(|plugin| plugin["name"].as_str())
        .collect();
    assert!(names.contains(&"keeper"), "{listed}");
}

#[test]
fn gather_reaches_changes_open_and_tasks_tree_through_the_host() {
    let board = board();
    board.write("changes/a.json", &change("Add a timeout", "build"));
    board.write(
        "tasks/one.md",
        "---\n$type: tasks/task/v2\ntitle: One\nstatus: open\n---\n",
    );
    let output = board.call("keeper.gather", &[]);
    assert_eq!(output["changes"]["total"], 1, "{output}");
    assert_eq!(output["changes"]["changes"][0]["path"], "/changes/a");
    assert_eq!(output["tasks"]["total"], 1, "{output}");
}

#[test]
fn render_writes_a_file_at_a_path_in_the_board() {
    let board = board();
    board.call("keeper.render", &[]);
    assert_eq!(
        board.read("projects/keeper/work-view/index.html"),
        "<!doctype html>\n"
    );
}
