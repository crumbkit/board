//! `board call <plugin>.<capability>` takes the capability's top-level
//! string, integer, number and boolean input properties as flags.

use super::plugins::{RESULT, installed};
use super::*;

/// A manifest whose capability takes four flag properties and one that
/// cannot be a flag.
const FLAGGED: &str = r#"{
  "$type": "board/plugin-manifest/v1",
  "name": "tasks",
  "version": "0.1.0",
  "description": "Open tasks.",
  "run": ["./run"],
  "capabilities": [{
    "name": "due",
    "description": "Open tasks due before a date.",
    "inputSchema": {"type": "object", "properties": {
      "before": {"type": "string", "description": "The date the tasks are due before."},
      "limit": {"type": "integer"},
      "ratio": {"type": "number"},
      "open": {"type": "boolean"},
      "tags": {"type": "array"}
    }},
    "outputSchema": {"type": "object"}
  }]
}"#;

/// A board enabling tasks, with tasks installed offering `due` with
/// `FLAGGED`'s input. The plugin saves its request beside its manifest.
fn flagged() -> (Scenario, Scenario) {
    let (board, data) = installed();
    let saved = data.workdir().join("plugins/tasks/request");
    let script = format!(
        "#!/bin/sh\ncat > '{}'\nprintf '%s' '{RESULT}'\n",
        saved.display()
    );
    data.write("plugins/tasks/plugin.json", Item::File(FLAGGED))
        .write("plugins/tasks/run", Item::Executable(&script));
    (board, data)
}

fn call(board: &Scenario, data: &Scenario, arguments: &[&str], stdin: &str) -> Output {
    let plugins = data.workdir().join("plugins");
    board_full(
        board.workdir(),
        None,
        Some(&plugins),
        arguments,
        stdin.as_bytes(),
    )
}

/// The input the plugin was sent.
fn sent(data: &Scenario) -> Value {
    let request = std::fs::read(data.workdir().join("plugins/tasks/request")).unwrap();
    serde_json::from_slice::<Value>(&request).unwrap()["input"].clone()
}

fn code(output: &Output) -> Value {
    json(output)["code"].clone()
}

#[test]
fn flags_send_the_same_input_as_json_on_stdin() {
    let (board, data) = flagged();
    let flags = [
        "call",
        "tasks.due",
        "--before",
        "2026-10-05",
        "--limit",
        "3",
        "--ratio",
        "0.5",
        "--open",
        "true",
    ];
    assert_eq!(call(&board, &data, &flags, "").code, 0);
    let expected = json!({"before": "2026-10-05", "limit": 3, "ratio": 0.5, "open": true});
    assert_eq!(sent(&data), expected);
    let stdin = expected.to_string();
    assert_eq!(call(&board, &data, &["call", "tasks.due"], &stdin).code, 0);
    assert_eq!(sent(&data), expected);
}

#[test]
fn no_flags_and_nothing_on_stdin_send_an_empty_object() {
    let (board, data) = flagged();
    assert_eq!(call(&board, &data, &["call", "tasks.due"], "").code, 0);
    assert_eq!(sent(&data), json!({}));
    assert_eq!(
        call(&board, &data, &["call", "--capability", "due"], "").code,
        0
    );
    assert_eq!(sent(&data), json!({}));
}

#[test]
fn with_a_flag_stdin_is_not_read() {
    let (board, data) = flagged();
    let flags = call(&board, &data, &["call", "tasks.due", "--limit", "3"], "[]");
    assert_eq!(flags.code, 0);
    assert_eq!(sent(&data), json!({"limit": 3}));
    let refused = call(&board, &data, &["--json", "call", "tasks.due"], "[]");
    assert_eq!(refused.code, 1);
    assert_eq!(code(&refused), "board::plugin::bad_input");
}

#[test]
fn a_flag_whose_value_is_not_its_type_is_a_refused_command_line() {
    let (board, data) = flagged();
    let refused = call(
        &board,
        &data,
        &["--json", "call", "tasks.due", "--limit", "x"],
        "",
    );
    assert_eq!(refused.code, 2);
    assert_eq!(code(&refused), "board::cli::usage");
    let admitted = call(
        &board,
        &data,
        &["--json", "call", "tasks.due", "--limit", "3"],
        "",
    );
    assert_eq!(admitted.code, 0);
}

#[test]
fn a_property_that_is_not_a_scalar_is_no_flag() {
    let (board, data) = flagged();
    let refused = call(
        &board,
        &data,
        &["--json", "call", "tasks.due", "--tags", "a"],
        "",
    );
    assert_eq!(refused.code, 2);
    assert_eq!(code(&refused), "board::cli::usage");
    let stdin = r#"{"tags": ["a"]}"#;
    assert_eq!(call(&board, &data, &["call", "tasks.due"], stdin).code, 0);
    assert_eq!(sent(&data), json!({"tags": ["a"]}));
}

#[test]
fn a_target_that_names_nothing_installed_keeps_boards_refusal() {
    let (board, data) = flagged();
    let nope = call(
        &board,
        &data,
        &["--json", "call", "nope.due", "--limit", "3"],
        "",
    );
    assert_eq!(code(&nope), "board::plugin::not_installed");
    let dew = call(&board, &data, &["call", "tasks.dew"], "{}");
    let stderr = String::from_utf8(dew.stderr).unwrap();
    assert!(
        stderr.contains("plugin tasks has no capability dew; it offers due"),
        "{stderr}"
    );
    let due = call(
        &board,
        &data,
        &["--json", "call", "tasks.due", "--limit", "3"],
        "",
    );
    assert_eq!(due.code, 0);
}

#[test]
fn help_lists_the_flags_of_an_installed_capability_and_refuses_a_target_like_a_run() {
    let (board, data) = flagged();
    let help = call(&board, &data, &["call", "tasks.due", "-h"], "");
    assert_eq!(help.code, 0);
    let text = String::from_utf8(help.stdout).unwrap();
    for flag in ["--before", "--limit", "--ratio", "--open"] {
        assert!(text.contains(flag), "{flag} in {text}");
    }
    assert!(
        text.contains("The date the tasks are due before."),
        "{text}"
    );
    assert!(!text.contains("--tags"), "{text}");
    assert!(text.contains("with one, it ignores stdin"), "{text}");
    assert!(text.contains("go on stdin: tags."), "{text}");
    let nope = call(&board, &data, &["--json", "call", "nope.due", "-h"], "");
    assert_eq!(code(&nope), "board::plugin::not_installed");
}

#[test]
fn a_terminal_on_stdin_is_not_read() {
    let (board, data) = flagged();
    let context = |terminal| Context {
        start: board.workdir().to_path_buf(),
        home: None,
        cache: None,
        plugins: Some(data.workdir().join("plugins")),
        boards: None,
        environment: board_plugin::Environment {
            path: Some("/usr/bin:/bin".into()),
            ..board_plugin::Environment::default()
        },
        complete: None,
        terminal,
        browser: None,
        binary: None,
        cursor: None,
    };
    let arguments = || ["board", "call", "tasks.due"].map(OsString::from);
    let mut torn: &[u8] = b"{";
    let read = run(arguments(), &context(false), &mut torn, &mut Ran::default());
    assert_eq!(read.error.as_deref(), Some("board::plugin::bad_input"));
    let mut torn: &[u8] = b"{";
    let unread = run(arguments(), &context(true), &mut torn, &mut Ran::default());
    assert_eq!(unread.code, 0);
    assert_eq!(sent(&data), json!({}));
    let flagged = ["board", "call", "tasks.due", "--limit", "3"].map(OsString::from);
    let ran = run(
        flagged,
        &context(false),
        &mut Unreadable,
        &mut Ran::default(),
    );
    assert_eq!(ran.code, 0);
    assert_eq!(sent(&data), json!({"limit": 3}));
    let nope = ["board", "call", "nope.due", "--limit", "3"].map(OsString::from);
    let ran = run(nope, &context(false), &mut Unreadable, &mut Ran::default());
    assert_eq!(ran.error.as_deref(), Some("board::plugin::not_installed"));
}

/// A stdin that fails every read, as a pipe nobody closes cannot be read.
struct Unreadable;

impl std::io::Read for Unreadable {
    fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
        Err(std::io::Error::other("stdin was read"))
    }
}

#[test]
fn the_reserved_flags_are_the_long_flags_of_a_grafted_call() {
    let (_, data) = flagged();
    data.write(
        "plugins/tasks/plugin.json",
        Item::File(&FLAGGED.replace(r#""properties""#, r#""unused""#)),
    );
    let arguments = ["board", "call", "tasks.due"].map(OsString::from);
    let plugins = data.workdir().join("plugins");
    let grafted = crate::graft::find(&arguments, Some(&plugins));
    assert!(grafted.is_some());
    let mut command = crate::graft::command(grafted.as_ref());
    command.build();
    let call = command.find_subcommand("call").unwrap();
    let leaf = call.find_subcommand("tasks.due").unwrap();
    let mut longs: Vec<&str> = call
        .get_arguments()
        .chain(leaf.get_arguments())
        .filter_map(clap::Arg::get_long)
        .collect();
    longs.sort_unstable();
    longs.dedup();
    let mut reserved = crate::graft::RESERVED_FLAGS.to_vec();
    reserved.sort_unstable();
    assert_eq!(longs, reserved);
}

#[test]
fn a_property_named_like_a_flag_of_call_goes_on_stdin() {
    let (board, data) = flagged();
    let manifest = FLAGGED.replace(r#""limit""#, r#""json""#);
    data.write("plugins/tasks/plugin.json", Item::File(&manifest));
    let flag = call(&board, &data, &["call", "tasks.due", "--json"], "");
    assert_eq!(flag.code, 0);
    assert_eq!(json(&flag)["data"]["output"], json!({"count": 2}));
    assert_eq!(sent(&data), json!({}));
    let stdin = r#"{"json": 3}"#;
    assert_eq!(call(&board, &data, &["call", "tasks.due"], stdin).code, 0);
    assert_eq!(sent(&data), json!({"json": 3}));
}

#[test]
fn a_property_whose_name_is_no_flag_name_goes_on_stdin() {
    let (board, data) = flagged();
    let manifest = FLAGGED
        .replace(r#""limit""#, r#""-x""#)
        .replace(r#""before""#, r#""by_2-a""#);
    data.write("plugins/tasks/plugin.json", Item::File(&manifest));
    let help = call(&board, &data, &["call", "tasks.due", "-h"], "");
    let text = String::from_utf8(help.stdout).unwrap();
    assert!(!text.contains("---x"), "{text}");
    assert!(text.contains("--by_2-a"), "{text}");
    let stdin = r#"{"-x": 3}"#;
    assert_eq!(call(&board, &data, &["call", "tasks.due"], stdin).code, 0);
    assert_eq!(sent(&data), json!({"-x": 3}));
}

#[test]
fn a_number_flag_keeps_its_spelling_and_refuses_what_json_cannot_hold() {
    let (board, data) = flagged();
    let whole = ["call", "tasks.due", "--ratio", "3"];
    assert_eq!(call(&board, &data, &whole, "").code, 0);
    assert_eq!(sent(&data).to_string(), r#"{"ratio":3}"#);
    let nan = call(
        &board,
        &data,
        &["--json", "call", "tasks.due", "--ratio", "nan"],
        "",
    );
    assert_eq!(nan.code, 2);
    assert_eq!(code(&nan), "board::cli::usage");
}

#[test]
fn an_empty_string_flag_is_sent() {
    let (board, data) = flagged();
    let empty = ["call", "tasks.due", "--before", ""];
    assert_eq!(call(&board, &data, &empty, "").code, 0);
    assert_eq!(sent(&data), json!({"before": ""}));
}

#[test]
fn whitespace_on_stdin_is_no_input() {
    let (board, data) = flagged();
    assert_eq!(call(&board, &data, &["call", "tasks.due"], "\n").code, 0);
    assert_eq!(sent(&data), json!({}));
}

#[test]
fn the_target_is_the_first_word_after_call_that_names_one() {
    let (board, data) = flagged();
    // `count` is the value of `--fields`, so `tasks.due` is the target, and
    // `--limit` parses as its flag. The run then refuses `--fields`, which
    // `call` does not take, where a missed graft would refuse `--limit`.
    let arguments = [
        "--json",
        "call",
        "--fields",
        "count",
        "tasks.due",
        "--limit",
        "3",
    ];
    let output = call(&board, &data, &arguments, "");
    assert_eq!(json(&output)["fields"]["argument"], "--fields");
}

#[test]
fn a_flag_without_a_description_has_its_type_as_help_and_call_help_names_flags() {
    let (board, data) = flagged();
    let help = call(&board, &data, &["call", "tasks.due", "-h"], "");
    let text = String::from_utf8(help.stdout).unwrap();
    assert!(text.contains("integer"), "{text}");
    let group = call(&board, &data, &["call", "-h"], "");
    let text = String::from_utf8(group.stdout).unwrap();
    assert!(text.contains("board call tasks.due -h"), "{text}");
}

#[test]
fn updating_a_parsed_command_line_replaces_its_graft() {
    use clap::FromArgMatches as _;
    let (_, data) = flagged();
    let plugins = data.workdir().join("plugins");
    let matches = |words: &[&str]| {
        let arguments: Vec<OsString> = words.iter().map(OsString::from).collect();
        let grafted = crate::graft::find(&arguments, Some(&plugins));
        crate::graft::command(grafted.as_ref()).get_matches_from(arguments)
    };
    let mut parsed = crate::graft::Parsed::from_arg_matches(&matches(&[
        "board",
        "call",
        "tasks.due",
        "--limit",
        "3",
    ]))
    .unwrap();
    assert!(parsed.grafted.is_some());
    parsed
        .update_from_arg_matches(&matches(&["board", "ls"]))
        .unwrap();
    assert!(parsed.grafted.is_none());
}

#[test]
fn a_target_like_word_before_call_is_not_the_target() {
    let (board, data) = flagged();
    let arguments = [
        "--json",
        "--fields",
        "a.b",
        "call",
        "tasks.due",
        "--limit",
        "3",
    ];
    let output = call(&board, &data, &arguments, "");
    assert_eq!(json(&output)["fields"]["argument"], "--fields");
}
