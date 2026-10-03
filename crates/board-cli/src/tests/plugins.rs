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

const RESULT: &str = r#"{"$type": "board/call-result/v1", "output": {"count": 2}}"#;

/// A script that ignores its request, writes `stdout` and a line of stderr,
/// and exits `exit`.
fn answering(stdout: &str, exit: u8) -> String {
    format!(
        "#!/bin/sh\ncat > /dev/null\necho 'a log line' >&2\nprintf '%s' '{stdout}'\nexit {exit}\n"
    )
}

/// Installs `name`, offering `due`, whose run script is `script`.
fn install(data: &Scenario, name: &str, script: &str) {
    data.write(
        &format!("plugins/{name}/plugin.json"),
        Item::File(&TASKS.replace(r#""tasks""#, &format!("{name:?}"))),
    )
    .write(&format!("plugins/{name}/run"), Item::Executable(script));
}

fn call(board: &Scenario, data: &Scenario, arguments: &[&str], input: &str) -> Output {
    let plugins = data.workdir().join("plugins");
    board_full(
        board.workdir(),
        None,
        Some(&plugins),
        arguments,
        input.as_bytes(),
    )
}

#[test]
fn call_prints_the_output_as_canonical_json() {
    let (board, data) = installed();
    install(&data, "tasks", &answering(RESULT, 0));
    let output = call(&board, &data, &["call", "tasks.due"], "{}");
    assert_eq!(output.code, 0, "{}", text(&output.stderr));
    assert_eq!(text(&output.stdout), "{\n  \"count\": 2\n}\n");
    let output = call(&board, &data, &["call", "--capability", "due"], "{}");
    assert_eq!(output.code, 0, "{}", text(&output.stderr));
}

#[test]
fn call_json_wraps_the_output_in_a_call_report() {
    let (board, data) = installed();
    install(&data, "tasks", &answering(RESULT, 0));
    let report = json(&call(&board, &data, &["--json", "call", "tasks.due"], "{}"));
    assert_eq!(
        report,
        serde_json::json!({
            "$type": "board/call-report/v1",
            "plugin": "tasks",
            "capability": "due",
            "output": {"count": 2}
        })
    );
}

#[test]
fn a_plugins_error_prints_as_its_own_code_with_its_stderr() {
    let (board, data) = installed();
    let error = r#"{"$type": "board/error/v1", "code": "tasks::date::bad", "message": "no such date", "fields": {"date": "x"}}"#;
    install(&data, "tasks", &answering(error, 1));
    let output = call(&board, &data, &["call", "tasks.due"], "{}");
    assert_eq!(output.code, 1);
    assert_eq!(
        text(&output.stderr),
        "error: tasks::date::bad: no such date\na log line\n"
    );
    let report = json(&call(&board, &data, &["--json", "call", "tasks.due"], "{}"));
    assert_eq!(report["$type"], "board/error/v1");
    assert_eq!(report["code"], "tasks::date::bad");
    assert_eq!(
        report["fields"],
        serde_json::json!({"date": "x", "plugin": "tasks"})
    );
}

#[test]
fn a_call_names_a_target_or_a_capability_but_not_both() {
    let (board, data) = installed();
    install(&data, "tasks", &answering(RESULT, 0));
    for refused in [
        &["call"][..],
        &["call", "tasks.due", "--capability", "due"],
        &["call", "tasks"],
    ] {
        assert_eq!(call(&board, &data, refused, "{}").code, 2, "{refused:?}");
    }
    assert_eq!(call(&board, &data, &["call", "tasks.due"], "{}").code, 0);
}

/// One refused call for each `board::plugin::` code a call can give in
/// under a second.
pub(super) fn refused_calls() -> Vec<Output> {
    let json_call = |board: &Scenario, data: &Scenario, arguments: &[&str], input: &str| {
        let mut arguments = arguments.to_vec();
        arguments.push("--json");
        call(board, data, &arguments, input)
    };
    let mut outputs = Vec::new();
    let (board, data) = installed();
    for (name, script) in [
        ("tasks", answering(RESULT, 0)),
        ("notes", answering(RESULT, 0)),
        ("bad", answering("not json", 0)),
        ("fails", answering("", 3)),
        (
            "huge",
            "#!/bin/sh\ncat > /dev/null\nhead -c 16777300 /dev/zero\n".to_owned(),
        ),
    ] {
        install(&data, name, &script);
    }
    outputs.push(json_call(&board, &data, &["call", "notes.due"], "{}"));
    outputs.push(json_call(&board, &data, &["call", "tasks.due"], "["));
    board.write(
        ".board/board.json",
        Item::File(
            r#"{"$type": "board/config/v1", "plugins": ["tasks", "notes", "bad", "fails", "huge"]}"#,
        ),
    );
    outputs.push(json_call(
        &board,
        &data,
        &["call", "--capability", "due"],
        "{}",
    ));
    outputs.push(json_call(&board, &data, &["call", "bad.due"], "{}"));
    outputs.push(json_call(&board, &data, &["call", "fails.due"], "{}"));
    outputs.push(json_call(&board, &data, &["call", "huge.due"], "{}"));
    let deep = board_plugin::Environment {
        depth: Some("8".into()),
        ..board_plugin::Environment::default()
    };
    outputs.push(board_environment(
        board.workdir(),
        None,
        Some(&data.workdir().join("plugins")),
        deep,
        &["call", "tasks.due", "--json"],
        b"{}",
    ));
    outputs
}
