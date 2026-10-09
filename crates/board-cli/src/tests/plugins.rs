//! `board plugin list`, `find` and `enable`.

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
    // Run with -C from elsewhere, a plugin that echoes its request sees the
    // board's root.
    let echo =
        "#!/bin/sh\nprintf '{\"$type\": \"board/call-result/v1\", \"output\": '\ncat\nprintf '}'\n";
    install(&data, "tasks", echo);
    let outside = Scenario::new();
    let root = board.workdir().canonicalize().unwrap();
    let arguments = ["-C", root.to_str().unwrap(), "--json", "call", "tasks.due"];
    let plugins = data.workdir().join("plugins");
    let echoed = board_full(outside.workdir(), None, Some(&plugins), &arguments, b"{}");
    assert_eq!(
        json(&echoed)["output"]["board"]["root"],
        root.to_str().unwrap(),
        "{}",
        text(&echoed.stderr)
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
fn a_plugin_that_fails_without_an_error_prints_boards_code_and_hint_then_its_stderr() {
    let (board, data) = installed();
    install(&data, "tasks", &answering("", 3));
    let output = call(&board, &data, &["call", "tasks.due"], "{}");
    assert_eq!(output.code, 1);
    let stderr = text(&output.stderr);
    let lines: Vec<&str> = stderr.lines().collect();
    assert_eq!(lines.len(), 3, "{stderr}");
    assert!(
        lines[0].starts_with("error: board::plugin::failed: "),
        "{stderr}"
    );
    assert!(lines[1].starts_with("hint: "), "{stderr}");
    assert_eq!(lines[2], "a log line");
    let report = json(&call(&board, &data, &["--json", "call", "tasks.due"], "{}"));
    assert_eq!(report["code"], "board::plugin::failed");
    assert!(report["hint"].is_string());
    assert_eq!(
        report["fields"],
        serde_json::json!({"plugin": "tasks", "stderr": "a log line\n"})
    );
    install(&data, "tasks", &answering(RESULT, 0));
    assert_eq!(call(&board, &data, &["call", "tasks.due"], "{}").code, 0);
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
    let sql = "SELECT name FROM p_tasks.sqlite_schema";
    let report = json(&call(&board, &data, &["--json", "query", sql], ""));
    assert_eq!(report["rows"], serde_json::json!([["board_database"]]));
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
    let attached = "SELECT name FROM p_reading_list.sqlite_schema";
    let output = call(&board, &data, &["--json", "query", attached], "");
    assert_eq!(
        json(&output)["rows"],
        serde_json::json!([["board_database"]])
    );
    let disabled = "SELECT count(*) FROM p_tasks.sqlite_schema";
    let output = call(&board, &data, &["--json", "query", disabled], "");
    assert_eq!(json(&output)["code"], "board::query::invalid");
}

/// A plugins directory holding notes, which ships the schema of
/// `notes/note/v1`, and a board that does not enable it.
/// A board that commits on put, as whom it says, and a plugins directory
/// holding notes, which ships `notes/note/v1`.
fn shipping() -> (Scenario, Scenario) {
    let board = Scenario::new();
    board
        .write(
            ".board/board.json",
            Item::File(r#"{"$type": "board/config/v1"}"#),
        )
        .write(
            ".git/config",
            Item::Appended("[user]\n\tname = Board Test\n\temail = board@example.com\n"),
        );
    let data = Scenario::new();
    data.write(
        "plugins/notes/plugin.json",
        Item::File(
            r#"{"$type": "board/plugin-manifest/v1", "name": "notes", "version": "1.0.0",
            "description": "Notes.", "run": ["./run"], "types": ["notes/note/v1"]}"#,
        ),
    )
    .write(
        "plugins/notes/types/notes/note/v1.schema.json",
        Item::File(r#"{"type": "object"}"#),
    );
    (board, data)
}

#[test]
fn plugin_list_prints_why_a_shipped_schema_is_unusable_and_a_usable_one_is_ok() {
    let (board, data) = shipping();
    data.write(
        "plugins/notes/types/notes/note/v1.schema.json",
        Item::File(r#"{"type": 3}"#),
    );
    let output = plugin(&board, &data, &["plugin", "list"]);
    assert_eq!(output.code, 1);
    let shipped = data
        .workdir()
        .join("plugins/notes/types/notes/note/v1.schema.json");
    let prefix = format!(
        "degraded board::plugin::bad_manifest notes disabled: the schema the plugin ships for \
         notes/note/v1 at {} does not compile: ",
        shipped.display()
    );
    let stdout = text(&output.stdout);
    assert!(stdout.starts_with(&prefix), "{stdout}");
    data.write(
        "plugins/notes/types/notes/note/v1.schema.json",
        Item::File(r#"{"type": "object"}"#),
    );
    let output = plugin(&board, &data, &["plugin", "list"]);
    assert_eq!(output.code, 0);
    assert_eq!(text(&output.stdout), "ok notes 1.0.0 disabled\n");
}

/// `plugin enable` refused because the board holds another schema.
pub(super) fn enable_over_another_schema() -> Output {
    let (board, data) = shipping();
    board.write(
        ".board/types/notes/note/v1.schema.json",
        Item::File(r#"{"type": "array"}"#),
    );
    plugin(&board, &data, &["--json", "plugin", "enable", "notes"])
}

#[test]
fn plugin_enable_writes_the_schema_and_the_config_and_says_what_it_did() {
    let (board, data) = shipping();
    let output = plugin(&board, &data, &["plugin", "enable", "notes"]);
    assert_eq!(output.code, 0, "{}", text(&output.stderr));
    let lines: Vec<String> = text(&output.stdout).lines().map(str::to_owned).collect();
    assert!(lines[0].starts_with("wrote /.board/types/notes/note/v1.schema "));
    assert!(lines[1].starts_with("wrote /.board/board "));
    assert!(lines[2].starts_with("committed "));
    assert_eq!(lines[3], "enabled notes");
    let again = plugin(&board, &data, &["plugin", "enable", "notes"]);
    assert_eq!(text(&again.stdout), "notes is already enabled\n");
    let report = json(&plugin(
        &board,
        &data,
        &["--json", "plugin", "enable", "notes"],
    ));
    assert_eq!(report["$type"], "board/plugin-enable-report/v1");
    assert_eq!(report["writes"], serde_json::json!([]));
    assert_eq!(report["commit"], serde_json::Value::Null);
    let listed = text(&plugin(&board, &data, &["plugin", "list"]).stdout);
    assert_eq!(listed, "ok notes 1.0.0 enabled\n");
}

#[test]
fn plugin_enable_over_another_schema_is_refused_with_a_hint_and_over_none_is_admitted() {
    let refused = enable_over_another_schema();
    assert_eq!(refused.code, 1);
    assert_eq!(json(&refused)["code"], "board::plugin::schema_differs");
    let (board, data) = shipping();
    assert_eq!(
        plugin(&board, &data, &["plugin", "enable", "notes"]).code,
        0
    );
}

/// A board naming notes in its configuration without its schema.
fn lacking_a_schema() -> (Scenario, Scenario) {
    let (board, data) = shipping();
    board.write(
        ".board/board.json",
        Item::File(r#"{"$type": "board/config/v1", "plugins": ["notes"]}"#),
    );
    (board, data)
}

/// `plugin list` on a board lacking an enabled plugin's schema.
pub(super) fn list_lacking_a_schema() -> Output {
    let (board, data) = lacking_a_schema();
    plugin(&board, &data, &["--json", "plugin", "list"])
}

#[test]
fn plugin_list_names_a_schema_an_enabled_plugin_lacks_and_is_clean_once_enabled() {
    let (board, data) = lacking_a_schema();
    let output = plugin(&board, &data, &["plugin", "list"]);
    assert_eq!(output.code, 1);
    assert_eq!(
        text(&output.stdout),
        "degraded board::plugin::schema_missing notes 1.0.0 enabled: \
         board::plugin::schema_missing .board/types/notes/note/v1.schema.json\n"
    );
    let report = json(&plugin(&board, &data, &["--json", "plugin", "list"]));
    assert_eq!(
        report["plugins"][0]["code"],
        "board::plugin::schema_missing"
    );
    assert_eq!(
        report["plugins"][0]["schemas"],
        serde_json::json!([{
            "type": "notes/note/v1",
            "path": ".board/types/notes/note/v1.schema.json",
            "code": "board::plugin::schema_missing",
        }])
    );

    assert_eq!(
        plugin(&board, &data, &["plugin", "enable", "notes"]).code,
        0
    );
    let output = plugin(&board, &data, &["--json", "plugin", "list"]);
    assert_eq!(output.code, 0);
    assert_eq!(json(&output)["plugins"][0].get("schemas"), None);
}

#[test]
fn plugin_enable_json_names_each_write_as_put_does() {
    let (board, data) = shipping();
    let report = json(&plugin(
        &board,
        &data,
        &["--json", "plugin", "enable", "notes"],
    ));
    assert_eq!(report["plugin"], "notes");
    let schema = &report["writes"][0];
    assert_eq!(schema["display"], "/.board/types/notes/note/v1.schema");
    assert_eq!(schema["previous"], serde_json::Value::Null);
    assert!(schema["blob"].is_string());
    assert!(report["writes"][1]["previous"].is_string());
    assert!(report["commit"].is_string());
}

#[test]
fn a_plugin_the_board_does_not_enable_is_named_in_the_hint_and_an_enabled_one_runs() {
    let (board, data) = installed();
    install(&data, "tasks", &answering(RESULT, 0));
    install(&data, "notes", &answering(RESULT, 0));
    let refused = call(&board, &data, &["call", "notes.due"], "{}");
    assert_eq!(
        hint_of(&refused),
        "add `notes` to `plugins` in `.board/board.json`"
    );
    assert_eq!(call(&board, &data, &["call", "tasks.due"], "{}").code, 0);
}

#[test]
fn a_capability_two_plugins_offer_hints_a_call_naming_one_and_that_call_runs() {
    let (board, data) = installed();
    board.write(
        ".board/board.json",
        Item::File(r#"{"$type": "board/config/v1", "plugins": ["tasks", "notes"]}"#),
    );
    install(&data, "tasks", &answering(RESULT, 0));
    install(&data, "notes", &answering(RESULT, 0));
    let refused = call(&board, &data, &["call", "--capability", "due"], "{}");
    assert_eq!(
        hint_of(&refused),
        "name the plugin as well, as in `board call notes.due`; \
         `board plugin find --capability due` lists the plugins that offer it"
    );
    let report = json(&call(
        &board,
        &data,
        &["--json", "call", "--capability", "due"],
        "{}",
    ));
    assert_eq!(
        report["fields"],
        serde_json::json!({"capability": "due", "plugins": ["notes", "tasks"]})
    );
    assert_eq!(call(&board, &data, &["call", "notes.due"], "{}").code, 0);
}

#[test]
fn input_the_capability_refuses_hints_how_to_see_its_schema_and_valid_input_runs() {
    let (board, data) = installed();
    install(&data, "tasks", &answering(RESULT, 0));
    let refused = call(&board, &data, &["call", "tasks.due"], "[]");
    assert_eq!(
        hint_of(&refused),
        "send JSON on stdin that matches the capability's `inputSchema`; \
         `board plugin find --capability due --json` shows it"
    );
    let shown = plugin(
        &board,
        &data,
        &["plugin", "find", "--capability", "due", "--json"],
    );
    assert_eq!(shown.code, 0);
    assert_eq!(call(&board, &data, &["call", "tasks.due"], "{}").code, 0);
}
