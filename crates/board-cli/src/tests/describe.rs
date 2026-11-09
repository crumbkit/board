//! `board commands list`, `board schemas list` and `board schemas show`
//! describe Board's own commands and documents, and every command takes
//! the flags a program adds when it runs one for a person.

use clap::CommandFactory as _;
use crumbkit_test::{Allow, Check, Examples, Rule, Suite};

use super::*;
use crate::run::Cli;

/// The flags a program adds before `--` when it runs a command for a
/// person: a JSON document, every field of every row, and no question.
const ADDED: [&str; 4] = ["--json", "--full", "--no-input", "--"];

/// The options and the positional words that make each leaf's command
/// line parse, in the order `commands list` lists the leaves.
const LINES: &[(&str, &[&str], &[&str])] = &[
    ("init", &[], &[]),
    ("check", &[], &[]),
    ("get", &[], &["/projects/p"]),
    ("ls", &[], &[]),
    ("refs", &[], &["/projects/p"]),
    ("uri", &[], &["/projects/p"]),
    ("put", &[], &["/tasks/a"]),
    ("rm", &[], &["/tasks/a"]),
    ("sync", &[], &[]),
    ("mount update", &[], &["/vendor/lib"]),
    ("plugin list", &[], &[]),
    ("plugin find", &[], &[]),
    ("plugin enable", &[], &["tasks"]),
    ("type add", &[], &["notes/note/v1"]),
    ("call", &[], &["tasks.available"]),
    (
        "decide",
        &["--op", "review.comment", "--path", "/reviews/r1"],
        &[],
    ),
    (
        "answer",
        &[],
        &[
            "4b2e0c8a9d1f37b6e5a0c2d84f9b1e7a3c6d0f8b2a5e9c1d4f7a0b3e6c9d2f5a",
            "approve",
        ],
    ),
    ("query", &[], &["SELECT 1"]),
    ("serve", &[], &[]),
    ("ui kinds", &[], &[]),
    ("ui template", &[], &["markdown"]),
    ("ui open", &[], &["/bindings/review"]),
    ("skills install", &[], &[]),
    ("skills list", &[], &[]),
    ("commands list", &[], &[]),
    ("schemas list", &[], &[]),
    ("schemas show", &[], &["board/check-report/v2"]),
];

/// The leaves whose JSON already holds every field, because they print
/// prose, headed answers or rows that print whole. They take `--full` only
/// with `--json`, and say on stderr that it adds nothing.
const WHOLE: &[&str] = &[
    "init",
    "check",
    "get",
    "ls",
    "refs",
    "uri",
    "put",
    "rm",
    "sync",
    "mount update",
    "plugin list",
    "plugin find",
    "plugin enable",
    "type add",
    "call",
    "decide",
    "answer",
    "query",
    "serve",
    "ui template",
    "ui open",
];

/// The command line of the leaf at `path` as a program builds it: the
/// path, the options, `flags`, then the positional words.
fn line(path: &str, options: &[&str], flags: &[&str], words: &[&str]) -> Vec<OsString> {
    std::iter::once(BINARY_NAME)
        .chain(path.split(' '))
        .chain(options.iter().copied())
        .chain(flags.iter().copied())
        .chain(words.iter().copied())
        .map(OsString::from)
        .collect()
}

/// Whether the command line parses as Board parses it, and the message of
/// its refusal when it does not.
fn parses(arguments: Vec<OsString>) -> Result<(), String> {
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let sink = &mut crumbkit::Sink::new(&mut stdout, &mut stderr);
    let parsed = crumbkit::try_parse_to::<crate::graft::Parsed>(
        sink,
        Cli::command(),
        &crate::render::TOOL,
        arguments,
    );
    match parsed {
        Ok(_) => Ok(()),
        Err(unparsed) => {
            let _code = unparsed.print_to(sink);
            Err(format!("{}{}", text(&stdout), text(&stderr)))
        }
    }
}

/// The path of each leaf below the root, as `board commands list` names it
/// after `board `.
fn leaves() -> Vec<String> {
    let rows = commands();
    let leaves = rows.iter().filter(|row| row["form"].is_string());
    let paths = leaves.filter_map(|row| row["path"].as_str()?.strip_prefix("board "));
    paths.map(str::to_owned).collect()
}

/// The rows `board commands list --json --full --all --no-input` prints
/// outside any board.
fn commands() -> Vec<Value> {
    let outside = Scenario::new();
    let output = board_in(
        outside.workdir(),
        &[
            "commands",
            "list",
            "--json",
            "--full",
            "--all",
            "--no-input",
        ],
    );
    assert_eq!(output.code, 0, "{}", text(&output.stderr));
    let document = json(&output);
    assert_eq!(document["schema"], "board/commands-list/v1");
    document["data"].as_array().unwrap().clone()
}

#[test]
fn commands_list_runs_outside_a_board_and_lists_every_command_with_its_effects() {
    let rows = commands();
    let leaves: Vec<&Value> = (rows.iter())
        .filter(|row| row["path"] != "board" && row["form"].is_string())
        .collect();
    let paths: Vec<&str> = (leaves.iter())
        .map(|row| row["path"].as_str().unwrap().trim_start_matches("board "))
        .collect();
    let expected: Vec<&str> = LINES.iter().map(|(path, ..)| *path).collect();
    assert_eq!(paths, expected);
    for row in &leaves {
        assert!(row["effects"].is_object(), "{} has no effects", row["path"]);
    }
    let effect = |path: &str| {
        let row = leaves.iter().find(|row| row["path"] == path).unwrap();
        (
            row["effects"]["read_only"].as_bool().unwrap(),
            row["effects"]["destructive"].as_bool().unwrap(),
        )
    };
    assert_eq!(effect("board get"), (true, false));
    assert_eq!(effect("board put"), (false, true));
    assert_eq!(effect("board rm"), (false, true));
    assert_eq!(effect("board call"), (false, true));
    assert_eq!(effect("board type add"), (false, false));
    let grouped = (crate::render::GOALS.goals().iter()).flat_map(crumbkit::help::Goal::commands);
    let mut grouped: Vec<&str> = grouped.copied().collect();
    grouped.sort_unstable();
    let mut roots: Vec<&str> = (rows.iter())
        .filter_map(|row| row["path"].as_str()?.strip_prefix("board "))
        .filter(|path| !path.contains(' '))
        .collect();
    roots.sort_unstable();
    assert_eq!(grouped, roots, "each root command is under one goal");
}

#[test]
fn root_help_lists_the_commands_under_their_goals() {
    let outside = Scenario::new();
    let help = text(&board_in(outside.workdir(), &["-h"]).stdout);
    for goal in crate::render::GOALS.goals() {
        assert!(help.contains(&format!("\n{}\n", goal.heading())), "{help}");
    }
}

#[test]
fn the_root_row_of_commands_list_names_its_goal_headings_and_a_group_without_goals_has_none() {
    let rows = commands();
    let row = |path: &str| rows.iter().find(|row| row["path"] == path).unwrap();
    let goals: Vec<(&str, Vec<&str>)> = (row("board")["goals"].as_array().unwrap().iter())
        .map(|goal| {
            let commands = goal["commands"].as_array().unwrap().iter();
            (
                goal["heading"].as_str().unwrap(),
                commands.map(|name| name.as_str().unwrap()).collect(),
            )
        })
        .collect();
    let expected: Vec<(&str, Vec<&str>)> = (crate::render::GOALS.goals().iter())
        .map(|goal| (goal.heading(), goal.commands().to_vec()))
        .collect();
    assert_eq!(goals, expected);
    assert_eq!(row("board plugin").get("goals"), None);
}

#[test]
fn commands_list_of_a_path_lists_that_branch_and_a_path_that_names_nothing_is_refused() {
    let outside = Scenario::new();
    let branch = board_in(outside.workdir(), &["commands", "list", "plugin", "--json"]);
    assert_eq!(branch.code, 0, "{}", text(&branch.stderr));
    let paths: Vec<Value> = (json(&branch)["data"].as_array().unwrap().iter())
        .map(|row| row["path"].clone())
        .collect();
    assert_eq!(
        paths,
        [
            "board plugin",
            "board plugin list",
            "board plugin find",
            "board plugin enable"
        ]
    );
    let refused = board_in(
        outside.workdir(),
        &["commands", "list", "plugins", "--json"],
    );
    assert_eq!(refused.code, 1, "{}", text(&refused.stdout));
    assert_eq!(json(&refused)["code"], "board::commands::not_found");
}

#[test]
fn schemas_list_names_each_document_board_prints_and_show_prints_its_schema() {
    let outside = Scenario::new();
    let listed = board_in(
        outside.workdir(),
        &["schemas", "list", "--json", "--full", "--all", "--no-input"],
    );
    assert_eq!(listed.code, 0, "{}", text(&listed.stderr));
    let ids: Vec<String> = (json(&listed)["data"].as_array().unwrap().iter())
        .map(|row| row["id"].as_str().unwrap().to_owned())
        .collect();
    for id in [
        "board/check-report/v2",
        "board/put-report/v2",
        "board/commands-list/v1",
        "board/schemas-show/v1",
    ] {
        assert!(
            ids.iter().any(|listed| listed == id),
            "{id} is not in {ids:?}"
        );
    }
    let shown = board_in(
        outside.workdir(),
        &[
            "schemas",
            "show",
            "--json",
            "--no-input",
            "--",
            "board/check-report/v2",
        ],
    );
    assert_eq!(shown.code, 0, "{}", text(&shown.stderr));
    let row = &json(&shown)["data"][0];
    assert_eq!(row["id"], "board/check-report/v2");
    assert!(row["schema"]["$schema"].is_string(), "{row}");
    let unknown = board_in(
        outside.workdir(),
        &[
            "schemas",
            "show",
            "--json",
            "--no-input",
            "--",
            "board/chek-report/v2",
        ],
    );
    assert_eq!(unknown.code, 1, "{}", text(&unknown.stdout));
    assert_eq!(
        json(&unknown)["failures"][0]["code"],
        "board::schemas::not_found"
    );
}

#[test]
fn every_leaf_takes_no_input_and_json_before_its_own_words() {
    let listed = leaves();
    let tabled: Vec<String> = LINES.iter().map(|(path, ..)| (*path).to_owned()).collect();
    assert_eq!(listed, tabled);
    for (path, options, words) in LINES {
        let without = line(path, options, &["--json", "--no-input", "--"], words);
        assert_eq!(parses(without), Ok(()), "board {path}");
    }
}

#[test]
fn every_leaf_takes_json_full_and_a_whole_leaf_refuses_full_without_json() {
    let mut whole = Vec::new();
    for (path, options, words) in LINES {
        assert_eq!(
            parses(line(path, options, &ADDED, words)),
            Ok(()),
            "board {path}"
        );
        let without_json = line(path, options, &["--full", "--no-input", "--"], words);
        let refusal = parses(without_json).unwrap_err();
        assert!(refusal.starts_with("error: --full "), "{refusal}");
        assert!(
            refusal.ends_with("  code:  board::cli::usage\n"),
            "{refusal}"
        );
        let help = refusal
            .lines()
            .find(|line| line.starts_with("  help:"))
            .unwrap();
        if help == "  help:  drop --full, because --json already prints the whole result" {
            whole.push(*path);
        } else {
            assert_eq!(help, "  help:  add --json, or drop --full", "board {path}");
        }
    }
    assert_eq!(whole, WHOLE);
}

/// Runs `command` with `--json` and with `--json` and `flag`, and checks that
/// `flag` changes nothing but the note it adds to stderr. Then runs it with
/// `flag` alone, which crumbkit refuses because the flag needs `--json`.
fn adds_nothing(scenario: &Scenario, command: &[&str], flag: &str) {
    let run = |flags: &[&str]| {
        let line: Vec<&str> = command.iter().chain(flags).copied().collect();
        board_in(scenario.workdir(), &line)
    };
    let alone = run(&["--json"]);
    let flagged = run(&["--json", flag]);
    assert_eq!(
        flagged.code,
        alone.code,
        "{command:?} {flag}: {}",
        text(&flagged.stderr)
    );
    assert_eq!(
        text(&flagged.stdout),
        text(&alone.stdout),
        "{command:?} {flag}"
    );
    let note = format!(
        "note: {flag} adds nothing to --json here; the JSON already holds the whole result.\n"
    );
    let stderr = text(&flagged.stderr);
    assert!(stderr.contains(&note), "{command:?} {flag}: {stderr}");
    assert_eq!(stderr.replacen(&note, "", 1), text(&alone.stderr));
    let refused = run(&[flag]);
    let refusal = text(&refused.stderr);
    assert_eq!(refused.code, 2, "{command:?} {flag}");
    assert!(refusal.starts_with(&format!("error: {flag} ")), "{refusal}");
    assert!(
        refusal.ends_with("  code:  board::cli::usage\n"),
        "{refusal}"
    );
}

#[test]
fn a_whole_leaf_prints_with_json_full_what_json_alone_prints_and_a_note_on_stderr() {
    let scenario = board();
    for command in [
        &["check"][..],
        &["get", "/projects/p"],
        &["ls", "/projects"],
        &["refs", "/projects/p"],
        &["plugin", "list"],
        &["query", "SELECT 1"],
    ] {
        adds_nothing(&scenario, command, "--full");
    }
}

#[test]
fn a_whole_row_leaf_prints_with_json_long_what_json_alone_prints_and_a_note_on_stderr() {
    let scenario = board();
    for command in [&["check"][..], &["ls", "/projects"]] {
        adds_nothing(&scenario, command, "--long");
    }
}

#[test]
fn no_input_before_the_command_is_admitted_and_a_misspelling_is_refused() {
    let outside = Scenario::new();
    let admitted = board_in(
        outside.workdir(),
        &["--no-input", "commands", "list", "--json"],
    );
    assert_eq!(admitted.code, 0, "{}", text(&admitted.stderr));
    let refused = board_in(
        outside.workdir(),
        &["--no-inputs", "commands", "list", "--json"],
    );
    assert_eq!(refused.code, 2, "{}", text(&refused.stderr));
    assert_eq!(json(&refused)["code"], "board::cli::usage");
}

/// Why a write that commits takes no `--yes`: it asks no question, and git
/// keeps the commit that a revert undoes.
const COMMITTED: &str = "the command asks no question, so it has no confirmation to skip, and git keeps each commit it makes, which a revert undoes";

/// Why `put` and `rm` keep their own `--expect`.
const BLOB: &str = "Board's --expect names the git blob the file must hold, or none for a new file, which crumbkit's Expect does not take";

/// Board's opt-outs from crumbkit's effects rules.
pub(super) const EFFECTS: &[Allow] = &[
    Allow::new(Rule::Yes, "put", COMMITTED),
    Allow::new(Rule::Yes, "rm", COMMITTED),
    Allow::new(Rule::Yes, "mount update", COMMITTED),
    Allow::new(
        Rule::Yes,
        "call",
        "the capability decides what it writes, and a plugin's op that the owner must admit already waits for the owner through decide",
    ),
    Allow::new(
        Rule::Yes,
        "answer",
        "an approval waits for the owner's Touch ID unless --signature gives a signature made elsewhere, --no-input refuses that wait, and a refusal writes only the answer",
    ),
    Allow::new(
        Rule::Yes,
        "skills install",
        "it replaces and removes only folders that hold Board's marker, which a second install writes again, and a folder without the marker only with --force",
    ),
    Allow::new(Rule::ExpectFlag, "put", BLOB),
    Allow::new(Rule::ExpectFlag, "rm", BLOB),
];

#[test]
fn every_command_matches_the_checked_in_command_reference() {
    // The reference describes `call` alone, so the check runs Board outside
    // any board, where `commands list` lists no plugin's capabilities. Its
    // test runs in this repository, whose board enables controls.
    let outside = Scenario::new();
    let script = format!(
        "#!/bin/sh\ncd '{}' && exec '{}' \"$@\"\n",
        outside.path(),
        super::help::binary()
    );
    outside.write("bin/board", Item::Executable(&script));
    let binary = outside.workdir().join("bin/board");
    Suite::new(super::conventions::build)
        .examples(Examples::new(binary.to_str().unwrap(), ""))
        .run(Check::CommandReference);
}

/// A plugin installed beside the built-in ones, with one capability that
/// adds idempotently within the board and one that reads, the first with an
/// enum flag.
const CHORES: &str = r#"{
  "$type": "board/plugin-manifest/v1",
  "name": "chores",
  "version": "0.1.0",
  "description": "Chores.",
  "run": ["./run"],
  "capabilities": [{
    "name": "add",
    "description": "Add a chore.",
    "inputSchema": {"type": "object", "additionalProperties": false, "properties": {
      "room": {"type": "string", "enum": ["kitchen", "hall"], "description": "Where."}
    }},
    "outputSchema": {"type": "object"},
    "annotations": {"readOnlyHint": false, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false}
  }, {
    "name": "list",
    "description": "List the chores.",
    "inputSchema": {"type": "object"},
    "outputSchema": {"type": "object"},
    "annotations": {"readOnlyHint": true}
  }]
}"#;

/// A board that enables `plugins`, beside a plugins directory holding
/// chores and a plugin whose manifest does not parse.
fn enabling(plugins: &str) -> (Scenario, Scenario) {
    let board = Scenario::new();
    let config = format!(r#"{{"$type": "board/config/v1", "plugins": [{plugins}]}}"#);
    board.write(".board/board.json", Item::File(&config));
    let data = Scenario::new();
    let hintless = super::plugins::TASKS.replace(r#""tasks""#, r#""hintless""#);
    data.write("plugins/chores/plugin.json", Item::File(CHORES))
        .write("plugins/hintless/plugin.json", Item::File(&hintless))
        .write("plugins/broken/plugin.json", Item::File("{}"));
    (board, data)
}

/// The `call <plugin>.<capability>` rows of `commands list` in `board`,
/// the row of the bare `call`, and what the run printed on stderr.
fn capability_rows(board: &Scenario, data: &Scenario) -> (Vec<Value>, Value, String) {
    let arguments = ["commands", "list", "call", "--json", "--full", "--all"];
    let (output, _) = super::built_in::built_in(board.workdir(), Some(data), &arguments);
    assert_eq!(output.code, 0, "{}", text(&output.stderr));
    let rows = json(&output)["data"].as_array().unwrap().clone();
    let bare = row(&rows, "board call").clone();
    let rows = rows
        .into_iter()
        .filter(|row| row["path"].as_str().unwrap().contains('.'));
    (rows.collect(), bare, text(&output.stderr))
}

/// The row of `rows` at `path`.
fn row<'a>(rows: &'a [Value], path: &str) -> &'a Value {
    let found = rows.iter().find(|row| row["path"] == path);
    found.unwrap_or_else(|| panic!("no {path} in {rows:#?}"))
}

#[test]
fn commands_list_in_a_board_enabling_a_built_in_plugin_lists_a_call_row_for_each_capability() {
    let (board, data) = enabling(r#""tasks""#);
    let (rows, bare, stderr) = capability_rows(&board, &data);
    assert_eq!(stderr, "");
    assert!(rows.iter().all(|row| {
        let path = row["path"].as_str().unwrap();
        path.starts_with("board call tasks.")
    }));
    let add = row(&rows, "board call tasks.add");
    assert_eq!(add["form"], "document");
    assert_eq!(add["columns"], bare["columns"]);
    assert_eq!(add["effects"]["destructive"], false);
    let flag = |row: &Value, name: &str| -> Value {
        let arguments = row["arguments"].as_array().unwrap();
        (arguments.iter().find(|argument| argument["name"] == name))
            .unwrap()
            .clone()
    };
    assert_eq!(
        flag(add, "--kind")["possible_values"],
        json!(["task", "project"])
    );
    let available = row(&rows, "board call tasks.available");
    assert!(flag(available, "--context")["help"].is_string());
}

#[test]
fn commands_list_lists_an_installed_plugins_capabilities_with_the_effects_their_hints_declare() {
    let (board, data) = enabling(r#""chores", "hintless", "tasks""#);
    let (rows, _, stderr) = capability_rows(&board, &data);
    assert_eq!(stderr, "");
    let paths: Vec<&Value> = (rows.iter().map(|row| &row["path"]))
        .filter(|path| path.as_str().unwrap().contains("chores"))
        .collect();
    assert_eq!(paths, ["board call chores.add", "board call chores.list"]);
    // read_only, destructive, idempotent, open_world
    let effects = |path: &str| {
        let effects = &row(&rows, path)["effects"];
        let names = ["read_only", "destructive", "idempotent", "open_world"];
        names.map(|name| effects[name].as_bool().unwrap())
    };
    assert_eq!(
        effects("board call chores.add"),
        [false, false, true, false]
    );
    assert_eq!(effects("board call chores.list"), [true, false, true, true]);
    assert_eq!(effects("board call tasks.add"), [false, false, false, true]);
    // hintless.due declares no hint, so it has MCP's defaults.
    assert_eq!(
        effects("board call hintless.due"),
        [false, true, false, true]
    );
    assert_eq!(row(&rows, "board call chores.add")["about"], "Add a chore.");
}

#[test]
fn commands_list_notes_a_plugin_it_cannot_read_and_only_it_and_schemas_read_manifests() {
    let (board, data) = enabling(r#""chores", "broken", "gone""#);
    let (rows, _, stderr) = capability_rows(&board, &data);
    assert_eq!(rows.len(), 2, "{rows:#?}");
    assert_eq!(
        stderr,
        "note: board::plugin::bad_manifest: plugin broken is skipped: the manifest is not a plugin manifest: missing field `$type`\n\
         note: board::plugin::not_installed: plugin gone is enabled but not installed, so it is skipped\n"
    );
    let phases = |arguments: &[&str]| {
        let (output, ran) = super::built_in::built_in(board.workdir(), Some(&data), arguments);
        assert_eq!(output.code, 0, "{}", text(&output.stderr));
        let phases = ran.trace.phases.iter().map(|phase| phase.name);
        (
            phases.collect::<Vec<_>>(),
            ran.trace.work.get("capabilities").copied(),
        )
    };
    for listing in [&["commands", "list"][..], &["schemas", "list"]] {
        let (phases, capabilities) = phases(listing);
        assert!(phases.contains(&"plugin.list"), "{listing:?}: {phases:?}");
        assert_eq!(capabilities, Some(2), "{listing:?}");
    }
    let (phases, capabilities) = phases(&["ls"]);
    assert!(!phases.contains(&"plugin.list"), "{phases:?}");
    assert_eq!(capabilities, None);
}

#[test]
fn commands_list_notes_why_a_board_that_exists_does_not_open_and_says_nothing_outside_a_board() {
    let (board, data) = enabling(r#""tasks""#);
    board.write(
        ".board/board.json",
        Item::File(r#"{"$type": "board/config/v1", "plugins": 1}"#),
    );
    let (rows, _, stderr) = capability_rows(&board, &data);
    assert!(rows.is_empty(), "{rows:#?}");
    assert!(
        stderr.starts_with("note: board::config::invalid: the board cannot be opened, so no plugin capability is listed: "),
        "{stderr}"
    );
    let outside = Scenario::new();
    let (rows, _, stderr) = capability_rows(&outside, &data);
    assert!(rows.is_empty(), "{rows:#?}");
    assert_eq!(stderr, "");
}

#[test]
fn commands_list_notes_an_unreadable_plugins_directory_and_lists_the_built_in_plugins() {
    let (board, _) = enabling(r#""tasks""#);
    let data = Scenario::new();
    data.write("plugins", Item::File("not a directory"));
    let (rows, _, stderr) = capability_rows(&board, &data);
    assert!(
        rows.iter().any(|row| row["path"] == "board call tasks.add"),
        "{rows:#?}"
    );
    assert!(
        stderr.starts_with("note: board::plugin::not_installed: the plugins directory cannot be read, so only built-in plugins are listed: "),
        "{stderr}"
    );
}

#[test]
fn commands_list_in_a_board_that_enables_no_plugin_lists_only_the_bare_call() {
    let (board, data) = enabling("");
    let (rows, _, stderr) = capability_rows(&board, &data);
    assert!(rows.is_empty(), "{rows:#?}");
    assert_eq!(stderr, "");
}

#[test]
fn each_capability_rows_command_line_parses_with_the_flags_a_program_adds() {
    let (board, data) = enabling(r#""tasks", "controls", "chores""#);
    let (rows, ..) = capability_rows(&board, &data);
    assert!(rows.len() > 2);
    let plugins = data.workdir().join("plugins");
    for row in &rows {
        let path = row["path"].as_str().unwrap().trim_start_matches("board ");
        let arguments = line(path, &[], &ADDED, &[]);
        let found = crate::graft::find(&arguments, board_plugins::BUILT_IN, Some(&plugins));
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let sink = &mut crumbkit::Sink::new(&mut stdout, &mut stderr);
        let parsed = crumbkit::try_parse_to::<crate::graft::Parsed>(
            sink,
            crate::graft::command(found.as_slice(), Cli::command()),
            &crate::graft::tool(found.as_slice()),
            arguments,
        );
        assert!(parsed.is_ok(), "{path}: {}{}", text(&stdout), text(&stderr));
    }
}

#[test]
fn an_enum_flag_lists_its_values_and_the_input_schema_refuses_another_value() {
    let (board, data) = enabling(r#""chores""#);
    let script = super::plugins::answering(super::plugins::RESULT, 0);
    data.write("plugins/chores/run", Item::Executable(&script));
    let call = |room: &str| {
        let arguments = ["call", "chores.add", "--room", room, "--json"];
        super::built_in::built_in(board.workdir(), Some(&data), &arguments).0
    };
    let refused = call("attic");
    assert_eq!(refused.code, 1, "{}", text(&refused.stdout));
    assert_eq!(json(&refused)["code"], "board::plugin::bad_input");
    let admitted = call("hall");
    assert_eq!(admitted.code, 0, "{}", text(&admitted.stderr));
    assert_eq!(json(&admitted)["data"]["output"], json!({"count": 2}));
    let (rows, ..) = capability_rows(&board, &data);
    let room = &row(&rows, "board call chores.add")["arguments"][0];
    assert_eq!(room["possible_values"], json!(["kitchen", "hall"]));
}
