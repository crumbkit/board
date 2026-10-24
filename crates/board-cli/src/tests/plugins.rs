//! `board plugin list`, `find` and `enable`.

use super::*;

pub(super) const TASKS: &str = r#"{
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
fn plugin_list_prints_each_plugin_with_the_names_of_its_capabilities() {
    let (board, data) = installed();
    let output = plugin(&board, &data, &["plugin", "list"]);
    assert_eq!(output.code, 0, "{}", text(&output.stderr));
    assert_eq!(text(&output.stdout), "ok tasks 0.1.0 enabled offers due\n");
}

/// A board enabling tasks and a missing plugin, beside a broken plugin.
fn degraded() -> (Scenario, Scenario) {
    let (board, data) = installed();
    board.write(
        ".board/board.json",
        Item::File(r#"{"$type": "board/config/v1", "plugins": ["tasks", "gone"]}"#),
    );
    data.write("plugins/broken/plugin.json", Item::File("{}"));
    (board, data)
}

#[test]
fn plugin_list_prints_its_line_per_plugin_and_long_adds_each_description() {
    let (board, data) = degraded();
    let output = plugin(&board, &data, &["plugin", "list"]);
    assert_eq!(output.code, 1);
    assert_eq!(
        text(&output.stdout),
        "degraded board::plugin::bad_manifest broken disabled: the manifest is not a plugin manifest: missing field `$type`\n\
         degraded board::plugin::not_installed gone enabled\n\
         ok tasks 0.1.0 enabled offers due\n"
    );
    let output = plugin(&board, &data, &["plugin", "list", "--long"]);
    assert_eq!(output.code, 1, "{}", text(&output.stderr));
    assert_eq!(
        text(&output.stdout),
        "STATUS                                 NAME    VERSION  ENABLED  OFFERS   DESCRIPTION  MESSAGE\n\
         degraded board::plugin::bad_manifest   broken  -        false    []       -            the manifest is not a plugin manifest: missing field `$type`\n\
         degraded board::plugin::not_installed  gone    -        true     []       -            -\n\
         ok                                     tasks   0.1.0    true     [\"due\"]  Open tasks.  -\n"
    );
}

#[test]
fn plugin_list_fields_admits_description_and_refuses_an_unknown_field() {
    let (board, data) = installed();
    let output = plugin(
        &board,
        &data,
        &["plugin", "list", "--fields", "name,description"],
    );
    assert_eq!(output.code, 0, "{}", text(&output.stderr));
    assert_eq!(
        text(&output.stdout),
        "NAME   DESCRIPTION\ntasks  Open tasks.\n"
    );
    let output = plugin(
        &board,
        &data,
        &["plugin", "list", "--fields", "status,code"],
    );
    assert_eq!(output.code, 0, "{}", text(&output.stderr));
    assert_eq!(text(&output.stdout), "STATUS  CODE\nok      -\n");
    let output = plugin(
        &board,
        &data,
        &["plugin", "list", "--fields", "name,summary"],
    );
    assert_eq!(output.code, 2);
    assert_eq!(text(&output.stdout), "");
    assert_eq!(
        text(&output.stderr),
        "error: --fields names an unknown field 'summary'\n  \
         help:  use the valid fields: capabilities, code, description, enabled, extract, message, metadata, name, offers, run, schemas, status, version\n  \
         code:  board::cli::usage\n"
    );
}

#[test]
fn plugin_list_json_adds_offers_and_is_otherwise_unchanged() {
    let (board, data) = degraded();
    let output = plugin(&board, &data, &["--json", "plugin", "list"]);
    assert_eq!(output.code, 1);
    assert_eq!(
        text(&output.stdout),
        concat!(
            r#"{"schema":"board/plugin-list-report/v2","data":["#,
            r#"{"code":"board::plugin::bad_manifest","enabled":false,"message":"the manifest is not a plugin manifest: missing field `$type`","name":"broken","offers":[],"status":"degraded"},"#,
            r#"{"code":"board::plugin::not_installed","enabled":true,"name":"gone","offers":[],"status":"degraded"},"#,
            r#"{"capabilities":[{"_meta":{},"description":"Open tasks due before a date.","inputSchema":{"type":"object"},"name":"due","outputSchema":{"type":"object"}}],"description":"Open tasks.","enabled":true,"extract":[],"metadata":{"area":"planning"},"name":"tasks","offers":["due"],"run":["./run"],"status":"ok","version":"0.1.0"}"#,
            r#"],"total":3,"truncated":false}"#,
            "\n"
        )
    );
}

#[test]
fn plugin_list_help_names_its_columns() {
    let (board, data) = installed();
    let output = plugin(&board, &data, &["plugin", "list", "-h"]);
    assert_eq!(output.code, 0);
    let help = text(&output.stdout);
    assert!(!help.contains("<ENTRY>"), "{help}");
    assert!(
        help.contains("<STATUS> <NAME> [<VERSION>] <ENABLED>[: <PROBLEMS>][ offers <OFFERS>]"),
        "{help}"
    );
    let output = plugin(&board, &data, &["plugin", "list", "--help"]);
    let help = text(&output.stdout);
    for column in [
        "status",
        "name",
        "version",
        "enabled",
        "offers",
        "description",
        "message",
        "schemas",
    ] {
        assert!(help.contains(&format!("  {column}")), "{column}: {help}");
    }
    assert!(
        help.contains("`board plugin find --capability <name>` describes each"),
        "{help}"
    );
    // The row decides its problem itself, because `code` is not a column.
    assert!(!help.contains("Problems:"), "{help}");
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
fn plugin_list_spends_its_limit_on_whole_plugins() {
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
        "ok tasks 0.1.0 enabled offers one, two\n",
        "ok zeta 0.1.0 disabled offers one, two\n",
    ];
    for limit in 1..=2 {
        let output = plugin(
            &board,
            &data,
            &["plugin", "list", "--limit", &limit.to_string()],
        );
        assert_eq!(text(&output.stdout), lines[..limit].concat(), "{limit}");
        assert_eq!(text(&output.stderr).is_empty(), limit == 2, "{limit}");
    }
}

#[test]
fn plugin_list_json_is_a_tagged_report_holding_the_manifest() {
    let (board, data) = installed();
    let report = json(&plugin(&board, &data, &["--json", "plugin", "list"]));
    assert_eq!(report["schema"], "board/plugin-list-report/v2");
    let tasks = &report["data"][0];
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
    let due = &report["data"][0]["capabilities"][0];
    assert_eq!(due["title"], "Due tasks");
    assert_eq!(
        due["annotations"],
        serde_json::json!({"readOnlyHint": true})
    );
    assert_eq!(due["_meta"], serde_json::json!({"area": "dates"}));

    let refused = full.replace(r#""readOnlyHint""#, r#""readOnly""#);
    data.write("plugins/tasks/plugin.json", Item::File(&refused));
    let report = json(&plugin(&board, &data, &["--json", "plugin", "list"]));
    assert_eq!(report["data"][0]["code"], "board::plugin::bad_manifest");
}

#[test]
fn plugin_list_fails_when_a_plugin_is_broken_or_missing() {
    let (board, data) = installed();
    data.write("plugins/broken/plugin.json", Item::File("{}"));
    let output = plugin(&board, &data, &["--json", "plugin", "list"]);
    assert_eq!(output.code, 1);
    let report = json(&output);
    assert_eq!(report["data"][0]["code"], "board::plugin::bad_manifest");
    assert_eq!(report["data"][1]["status"], "ok");

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
        "note: no capability is installed, or none matches every filter given\n"
    );
}

#[test]
fn plugin_find_lists_every_capability_without_a_filter_and_a_filter_narrows_it() {
    let (board, data) = installed();
    data.write(
        "plugins/tasks/plugin.json",
        Item::File(&two_capabilities("tasks")),
    )
    .write(
        "plugins/zeta/plugin.json",
        Item::File(&two_capabilities("zeta")),
    )
    .write("plugins/broken/plugin.json", Item::File("{}"));
    let output = plugin(&board, &data, &["plugin", "find"]);
    assert_eq!(output.code, 0, "{}", text(&output.stderr));
    assert_eq!(
        text(&output.stdout),
        "tasks.one enabled one.\ntasks.two enabled two.\n\
         zeta.one disabled one.\nzeta.two disabled two.\n"
    );
    let output = plugin(&board, &data, &["plugin", "find", "--capability", "two"]);
    assert_eq!(output.code, 0);
    assert_eq!(
        text(&output.stdout),
        "tasks.two enabled two.\nzeta.two disabled two.\n"
    );
}

#[test]
fn plugin_find_without_a_filter_says_when_no_capability_is_installed() {
    let (board, data) = installed();
    data.write("plugins/tasks", Item::Deleted);
    let output = plugin(&board, &data, &["plugin", "find"]);
    assert_eq!((output.code, text(&output.stdout)), (0, String::new()));
    assert_eq!(
        text(&output.stderr),
        "note: no capability is installed, or none matches every filter given\n"
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
    assert_eq!(report["schema"], "board/plugin-find-report/v2");
    let found = &report["data"][0];
    assert_eq!(
        (&found["plugin"], &found["capability"], &found["version"]),
        (&"tasks".into(), &"due".into(), &"0.1.0".into())
    );
}

/// The tasks manifest with a capability that has a title, annotations and
/// `_meta`.
fn titled() -> (Scenario, Scenario) {
    let (board, data) = installed();
    let full = TASKS.replace(
        r#""description": "Open tasks due before a date.","#,
        r#""title": "Due tasks",
    "description": "Open tasks due before a date.",
    "annotations": {"readOnlyHint": true, "destructiveHint": false},
    "_meta": {"area": "dates"},"#,
    );
    data.write("plugins/tasks/plugin.json", Item::File(&full));
    (board, data)
}

#[test]
fn plugin_find_prints_its_line_per_capability_and_long_adds_title_version_and_annotations() {
    let (board, data) = titled();
    let output = plugin(&board, &data, &["plugin", "find"]);
    assert_eq!(output.code, 0, "{}", text(&output.stderr));
    assert_eq!(
        text(&output.stdout),
        "tasks.due enabled Open tasks due before a date.\n"
    );
    let output = plugin(&board, &data, &["plugin", "find", "--long"]);
    assert_eq!(output.code, 0, "{}", text(&output.stderr));
    assert_eq!(
        text(&output.stdout),
        "PLUGIN  CAPABILITY  ENABLED  DESCRIPTION                    TITLE      VERSION  ANNOTATIONS\n\
         tasks   due         true     Open tasks due before a date.  Due tasks  0.1.0    {\"destructiveHint\":false,\"readOnlyHint\":true}\n"
    );
}

#[test]
fn plugin_find_fields_admits_its_fields_and_refuses_an_unknown_field() {
    let (board, data) = titled();
    let output = plugin(
        &board,
        &data,
        &["plugin", "find", "--fields", "capability,_meta,inputSchema"],
    );
    assert_eq!(output.code, 0, "{}", text(&output.stderr));
    assert_eq!(
        text(&output.stdout),
        "CAPABILITY  _META             INPUTSCHEMA\n\
         due         {\"area\":\"dates\"}  {\"type\":\"object\"}\n"
    );
    let output = plugin(
        &board,
        &data,
        &["plugin", "find", "--fields", "capability,summary"],
    );
    assert_eq!(output.code, 2);
    assert_eq!(text(&output.stdout), "");
    assert_eq!(
        text(&output.stderr),
        "error: --fields names an unknown field 'summary'\n  \
         help:  use the valid fields: _meta, annotations, capability, description, enabled, inputSchema, outputSchema, plugin, title, version\n  \
         code:  board::cli::usage\n"
    );
}

#[test]
fn plugin_find_json_prints_each_capability_whole_in_key_order() {
    let (board, data) = titled();
    data.write(
        "plugins/zeta/plugin.json",
        Item::File(&two_capabilities("zeta")),
    );
    let output = plugin(&board, &data, &["--json", "plugin", "find"]);
    assert_eq!(output.code, 0, "{}", text(&output.stderr));
    assert_eq!(
        text(&output.stdout),
        concat!(
            r#"{"schema":"board/plugin-find-report/v2","data":["#,
            r#"{"_meta":{"area":"dates"},"annotations":{"destructiveHint":false,"readOnlyHint":true},"capability":"due","description":"Open tasks due before a date.","enabled":true,"inputSchema":{"type":"object"},"outputSchema":{"type":"object"},"plugin":"tasks","title":"Due tasks","version":"0.1.0"},"#,
            r#"{"_meta":{},"capability":"one","description":"one.","enabled":false,"inputSchema":{"type":"object"},"outputSchema":{"type":"object"},"plugin":"zeta","version":"0.1.0"},"#,
            r#"{"_meta":{},"capability":"two","description":"two.","enabled":false,"inputSchema":{"type":"object"},"outputSchema":{"type":"object"},"plugin":"zeta","version":"0.1.0"}"#,
            r#"],"total":3,"truncated":false}"#,
            "\n"
        )
    );
}

#[test]
fn plugin_find_help_names_its_columns() {
    let (board, data) = installed();
    let output = plugin(&board, &data, &["plugin", "find", "-h"]);
    assert_eq!(output.code, 0);
    let help = text(&output.stdout);
    assert!(!help.contains("<ENTRY>"), "{help}");
    assert!(
        help.contains("<PLUGIN>.<CAPABILITY> <ENABLED> <DESCRIPTION>"),
        "{help}"
    );
    let output = plugin(&board, &data, &["plugin", "find", "--help"]);
    let help = text(&output.stdout);
    for column in [
        "plugin",
        "capability",
        "enabled",
        "description",
        "title",
        "version",
        "annotations",
    ] {
        assert!(help.contains(&format!("  {column}")), "{column}: {help}");
    }
}

#[test]
fn a_capability_or_filter_that_cannot_be_read_refuses_the_command_line() {
    let (board, data) = installed();
    for refused in [
        &["plugin", "find", "--capability", "Due"][..],
        &["plugin", "find", "--meta", "area"],
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

pub(super) const RESULT: &str = r#"{"$type": "board/call-result/v1", "output": {"count": 2}}"#;

/// A script that ignores its request, writes `stdout` and a line of stderr,
/// and exits `exit`.
pub(super) fn answering(stdout: &str, exit: u8) -> String {
    format!(
        "#!/bin/sh\ncat > /dev/null\necho 'a log line' >&2\nprintf '%s' '{stdout}'\nexit {exit}\n"
    )
}

/// Installs `name`, offering `due`, whose run script is `script`.
pub(super) fn install(data: &Scenario, name: &str, script: &str) {
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
            "schema": "board/call-report/v2",
            "data": {"plugin": "tasks", "capability": "due", "output": {"count": 2}},
            "failures": []
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
        json(&echoed)["data"]["output"]["board"]["root"],
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
        "error: no such date\n  \
         help:  no next step: the plugin gave no hint\n  \
         code:  tasks::date::bad\na log line\n"
    );
    let report = json(&call(&board, &data, &["--json", "call", "tasks.due"], "{}"));
    assert_eq!(report["schema"], "board/failure/v1");
    assert_eq!(report["code"], "tasks::date::bad");
    assert_eq!(
        report["fields"],
        serde_json::json!({"date": "x", "plugin": "tasks", "stderr": "a log line\n"})
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
    assert_eq!(lines.len(), 4, "{stderr}");
    assert!(lines[0].starts_with("error: "), "{stderr}");
    assert!(lines[1].starts_with("  help:  "), "{stderr}");
    assert_eq!(lines[2], "  code:  board::plugin::failed");
    assert_eq!(lines[3], "a log line");
    let report = json(&call(&board, &data, &["--json", "call", "tasks.due"], "{}"));
    assert_eq!(report["code"], "board::plugin::failed");
    assert!(report["recovery"]["text"].is_string());
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
    assert_eq!(report["data"], serde_json::json!([["board_database"]]));
    assert_eq!(report["summary"]["plugins"][0]["plugin"], "tasks");
    assert_eq!(
        report["summary"]["plugins"][0]["code"],
        "board::plugin::failed"
    );
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
    assert_eq!(report["summary"]["plugins"], serde_json::json!([]));
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
        json(&output)["data"],
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
pub(super) fn shipping() -> (Scenario, Scenario) {
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

/// A board enabling tasks, which ships `tasks/note/v1` as `{"type":
/// "object"}`, while the board holds `held` for it.
pub(super) fn holding(held: &str) -> (Scenario, Scenario) {
    let (board, data) = installed();
    let manifest = TASKS.replace(r#""run""#, r#""types": ["tasks/note/v1"], "run""#);
    install(&data, "tasks", &answering(RESULT, 0));
    data.write("plugins/tasks/plugin.json", Item::File(&manifest))
        .write(
            "plugins/tasks/types/tasks/note/v1.schema.json",
            Item::File(r#"{"type": "object"}"#),
        );
    board
        .write(".board/types/tasks/note/v1.schema.json", Item::File(held))
        .write(
            ".git/config",
            Item::Appended("[user]\n\tname = Board Test\n\temail = board@example.com\n"),
        );
    (board, data)
}

const REPLACED: &str = "note: replaced .board/types/tasks/note/v1.schema.json with the schema \
     the tasks plugin ships; records its old copy admitted may now be degraded, and \
     `board check` lists them\n";

#[test]
fn a_call_refused_for_a_differing_schema_says_to_enable_again_which_replaces_it() {
    let (board, data) = holding(r#"{"type": "array"}"#);
    let refused = call(&board, &data, &["--json", "call", "tasks.due"], "{}");
    assert_eq!(refused.code, 1);
    let report = json(&refused);
    assert_eq!(report["code"], "board::plugin::schema_differs");
    let hint = report["recovery"]["text"].as_str().unwrap();
    assert!(hint.contains("`board plugin enable tasks`"), "{hint}");
    let enabled = plugin(&board, &data, &["plugin", "enable", "tasks"]);
    assert_eq!(enabled.code, 0, "{}", text(&enabled.stderr));
    let lines: Vec<String> = text(&enabled.stdout).lines().map(str::to_owned).collect();
    assert!(lines[0].starts_with("wrote /.board/types/tasks/note/v1.schema "));
    assert!(lines[1].starts_with("committed "));
    assert_eq!(lines[2], "enabled tasks");
    assert_eq!(text(&enabled.stderr), REPLACED);
    assert_eq!(call(&board, &data, &["call", "tasks.due"], "{}").code, 0);
}

#[test]
fn plugin_enable_reports_a_schema_it_replaced_and_none_when_the_schema_is_the_same() {
    let (board, data) = holding(r#"{"type": "array"}"#);
    let enabled = plugin(&board, &data, &["--json", "plugin", "enable", "tasks"]);
    assert_eq!(text(&enabled.stderr), REPLACED);
    assert_eq!(
        json(&enabled)["data"]["replaced"],
        serde_json::json!([".board/types/tasks/note/v1.schema.json"])
    );
    let (board, data) = holding(r#"{"type": "object"}"#);
    let enabled = plugin(&board, &data, &["--json", "plugin", "enable", "tasks"]);
    assert_eq!(text(&enabled.stderr), "");
    assert_eq!(json(&enabled)["data"]["replaced"], serde_json::json!([]));
    let again = plugin(&board, &data, &["plugin", "enable", "tasks"]);
    assert_eq!(text(&again.stdout), "tasks is already enabled\n");
    assert_eq!(text(&again.stderr), "");
}

/// A board naming notes in its configuration without its schema.
pub(super) fn lacking_a_schema() -> (Scenario, Scenario) {
    let (board, data) = shipping();
    board.write(
        ".board/board.json",
        Item::File(r#"{"$type": "board/config/v1", "plugins": ["notes"]}"#),
    );
    (board, data)
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
    assert_eq!(report["data"][0]["code"], "board::plugin::schema_missing");
    assert_eq!(
        report["data"][0]["schemas"],
        serde_json::json!([{
            "type": "notes/note/v1",
            "path": ".board/types/notes/note/v1.schema.json",
            "code": "board::plugin::schema_missing",
        }])
    );
    let output = plugin(&board, &data, &["plugin", "list", "--long"]);
    assert_eq!(output.code, 1);
    let long = text(&output.stdout);
    assert!(
        long.lines()
            .next()
            .is_some_and(|header| header.ends_with("  SCHEMAS")),
        "{long}"
    );
    assert!(
        long.lines().nth(1).is_some_and(|row| row.ends_with(
            "  [{\"code\":\"board::plugin::schema_missing\",\"path\":\".board/types/notes/note/v1.schema.json\",\"type\":\"notes/note/v1\"}]"
        )),
        "{long}"
    );

    assert_eq!(
        plugin(&board, &data, &["plugin", "enable", "notes"]).code,
        0
    );
    let output = plugin(&board, &data, &["--json", "plugin", "list"]);
    assert_eq!(output.code, 0);
    assert_eq!(json(&output)["data"][0].get("schemas"), None);
}

#[test]
fn plugin_enable_json_names_each_write_as_put_does() {
    let (board, data) = shipping();
    let report = json(&plugin(
        &board,
        &data,
        &["--json", "plugin", "enable", "notes"],
    ));
    let report = &report["data"];
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
        "give the input as the flags that `board call tasks.due -h` lists, or as JSON \
         on stdin that matches the capability's `inputSchema`; `board \
         plugin find --capability due --json` shows the schema"
    );
    let shown = call(&board, &data, &["call", "tasks.due", "-h"], "");
    assert_eq!(shown.code, 0);
    let schema = ["plugin", "find", "--capability", "due", "--json"];
    assert_eq!(call(&board, &data, &schema, "").code, 0);
    assert_eq!(call(&board, &data, &["call", "tasks.due"], "{}").code, 0);
}

/// A call whose plugin database fell back to a temporary file says so on
/// stderr, in text and in JSON; the same call with its database beside the
/// index prints no note.
#[test]
fn a_call_whose_database_is_temporary_notes_it_and_one_beside_the_index_does_not() {
    let note = "note: plugin tasks's database is a temporary file for this run: \
                no directory for it can be made beside the index\n";
    for json in [false, true] {
        let stderr = |fallback| {
            let called = board_ops::Called {
                report: board_plugin::CallReport {
                    plugin: "tasks".parse().unwrap(),
                    capability: "due".parse().unwrap(),
                    output: json!({"count": 2}),
                },
                fallback,
            };
            let printer = crate::render::Printer::of(json, 1 << 20, &["call", "tasks.due"]);
            text(&crate::render::call(printer, &called).stderr)
        };
        assert_eq!(stderr(true), note, "json: {json}");
        assert_eq!(stderr(false), "", "json: {json}");
    }
}
