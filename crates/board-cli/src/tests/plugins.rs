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
    "inputSchema": {"type": "object"},
    "outputSchema": {"type": "object"}
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
fn plugin_list_counts_a_line_per_plugin_and_capability_so_a_limit_cuts_within_a_plugin() {
    let (board, data) = installed();
    let cut = plugin(&board, &data, &["plugin", "list", "--limit", "1"]);
    assert_eq!(text(&cut.stdout), "ok tasks 0.1.0 enabled\n");
    assert_eq!(
        text(&cut.stderr),
        "note: showing 1 of 2 plugins and capabilities; \
         --limit <n> (at most 10000) or --all shows more\n"
    );
    let report = json(&plugin(
        &board,
        &data,
        &["--json", "plugin", "list", "--limit", "1"],
    ));
    assert_eq!(report["plugins"][0]["capabilities"], serde_json::json!([]));
    assert_eq!(
        (report["total"].clone(), report["truncated"].clone()),
        (2.into(), true.into())
    );
    let whole = plugin(&board, &data, &["plugin", "list", "--limit", "2"]);
    assert_eq!(text(&whole.stdout).lines().count(), 2);
    assert_eq!(text(&whole.stderr), "");
}

/// A manifest for `name` offering capabilities `one` and `two`.
fn two_capabilities(name: &str) -> String {
    let capability = |capability: &str| {
        format!(
            r#"{{"name": "{capability}", "description": "{capability}.",
            "inputSchema": {{"type": "object"}}, "outputSchema": {{"type": "object"}}}}"#
        )
    };
    format!(
        r#"{{"$type": "board/plugin-manifest/v1", "name": "{name}", "version": "0.1.0",
        "description": "{name}.", "run": ["./run"],
        "capabilities": [{}, {}]}}"#,
        capability("one"),
        capability("two")
    )
}

#[test]
fn plugin_list_spends_its_limit_on_each_plugin_and_its_capabilities_in_turn() {
    let (board, data) = installed();
    data.write(
        "plugins/tasks/plugin.json",
        Item::File(&two_capabilities("tasks")),
    )
    .write(
        "plugins/zeta/plugin.json",
        Item::File(&two_capabilities("zeta")),
    );
    let lines = [
        "ok tasks 0.1.0 enabled\n",
        "  one one.\n",
        "  two two.\n",
        "ok zeta 0.1.0 disabled\n",
        "  one one.\n",
        "  two two.\n",
    ];
    for limit in 1..=6 {
        let output = plugin(
            &board,
            &data,
            &["plugin", "list", "--limit", &limit.to_string()],
        );
        assert_eq!(text(&output.stdout), lines[..limit].concat(), "{limit}");
        assert_eq!(text(&output.stderr).is_empty(), limit == 6, "{limit}");
    }
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
        tasks["capabilities"][0]["inputSchema"],
        serde_json::json!({"type": "object"})
    );
}

#[test]
fn plugin_list_json_prints_a_capabilitys_title_annotations_and_meta() {
    let (board, data) = installed();
    let full = TASKS.replace(
        r#""description": "Open tasks due before a date.","#,
        r#""title": "Due tasks",
    "description": "Open tasks due before a date.",
    "annotations": {"readOnlyHint": true},
    "_meta": {"area": "dates"},"#,
    );
    data.write("plugins/tasks/plugin.json", Item::File(&full));
    let report = json(&plugin(&board, &data, &["--json", "plugin", "list"]));
    let due = &report["plugins"][0]["capabilities"][0];
    assert_eq!(due["title"], "Due tasks");
    assert_eq!(
        due["annotations"],
        serde_json::json!({"readOnlyHint": true})
    );
    assert_eq!(due["_meta"], serde_json::json!({"area": "dates"}));

    let refused = full.replace(r#""readOnlyHint""#, r#""readOnly""#);
    data.write("plugins/tasks/plugin.json", Item::File(&refused));
    let report = json(&plugin(&board, &data, &["--json", "plugin", "list"]));
    assert_eq!(report["plugins"][0]["code"], "board::plugin::bad_manifest");
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
    assert_eq!(
        text(&output.stderr),
        "note: no installed capability matches every filter\n"
    );
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

#[test]
fn query_reports_a_plugin_that_did_not_extract_and_still_answers() {
    let (board, data) = installed();
    board
        .write(
            ".board/types/lr/task/v1.schema.json",
            Item::File(r#"{"type": "object"}"#),
        )
        .write("tasks/a.json", Item::File(r#"{"$type": "lr/task/v1"}"#));
    data.write(
        "plugins/tasks/plugin.json",
        Item::File(&TASKS.replace(r#""run""#, r#""extract": ["lr/task/v1"], "run""#)),
    )
    .write("plugins/tasks/run", Item::Executable(&answering("", 3)));
    let sql = "SELECT count(*) FROM p_tasks.sqlite_schema";
    let report = json(&call(&board, &data, &["--json", "query", sql], ""));
    assert_eq!(report["rows"], serde_json::json!([[0]]));
    assert_eq!(report["plugins"][0]["plugin"], "tasks");
    assert_eq!(report["plugins"][0]["code"], "board::plugin::failed");
    let output = call(&board, &data, &["query", sql], "");
    assert_eq!(output.code, 0);
    assert!(
        text(&output.stderr).contains("note: plugin tasks did not extract: board::plugin::failed"),
        "{}",
        text(&output.stderr)
    );

    data.write("plugins/tasks/run", Item::Executable(
        "#!/bin/sh\ncat > /dev/null\nprintf '%s' '{\"$type\": \"board/extract-result/v1\", \"blobs\": []}'\n",
    ));
    let report = json(&call(&board, &data, &["--json", "query", sql], ""));
    assert_eq!(report["plugins"], serde_json::json!([]));
}

#[test]
fn query_attaches_each_enabled_plugins_database_with_hyphens_as_underscores() {
    let (board, data) = installed();
    board.write(
        ".board/board.json",
        Item::File(r#"{"$type": "board/config/v1", "plugins": ["reading-list"]}"#),
    );
    install(&data, "reading-list", &answering(RESULT, 0));
    let attached = "SELECT count(*) FROM p_reading_list.sqlite_schema";
    let output = call(&board, &data, &["--json", "query", attached], "");
    assert_eq!(json(&output)["rows"], serde_json::json!([[0]]));
    let disabled = "SELECT count(*) FROM p_tasks.sqlite_schema";
    let output = call(&board, &data, &["--json", "query", disabled], "");
    assert_eq!(json(&output)["code"], "board::query::invalid");
}
