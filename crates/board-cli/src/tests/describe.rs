//! `board commands list`, `board schemas list` and `board schemas show`
//! describe Board's own commands and documents, and every command takes
//! the flags a program adds when it runs one for a person.

use clap::CommandFactory as _;
use crumbkit_test::{Allow, Rule};

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
    let parsed = crumbkit::try_parse_to::<crate::graft::Parsed, _, _, _, _>(
        sink,
        Cli::command(),
        &crate::render::GUIDE,
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

/// The note crumbkit prints on stderr when `--full` adds nothing to `--json`.
const ADDS_NOTHING: &str =
    "note: --full adds nothing to --json here; the JSON already holds the whole result.\n";

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
        let run = |flags: &[&str]| {
            let line: Vec<&str> = command
                .iter()
                .copied()
                .chain(flags.iter().copied())
                .collect();
            board_in(scenario.workdir(), &line)
        };
        let alone = run(&["--json"]);
        let full = run(&["--json", "--full"]);
        assert_eq!(full.code, alone.code, "{command:?}: {}", text(&full.stderr));
        assert_eq!(text(&full.stdout), text(&alone.stdout), "{command:?}");
        let stderr = text(&full.stderr);
        assert!(stderr.contains(ADDS_NOTHING), "{command:?}: {stderr}");
        assert_eq!(stderr.replacen(ADDS_NOTHING, "", 1), text(&alone.stderr));
        let refused = run(&["--full"]);
        assert_eq!(refused.code, 2, "{command:?}");
        assert!(
            text(&refused.stderr).ends_with("  code:  board::cli::usage\n"),
            "{command:?}: {}",
            text(&refused.stderr)
        );
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
const EFFECTS: &[Allow] = &[
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
    Allow::new(Rule::ExpectFlag, "put", BLOB),
    Allow::new(Rule::ExpectFlag, "rm", BLOB),
];

#[test]
fn every_command_declares_its_effects() {
    crumbkit_test::check_effects(&Cli::command(), &crate::render::GUIDE, EFFECTS);
}

#[test]
fn commands_and_schemas_sit_at_crumbkits_fixed_paths() {
    crumbkit_test::check_mounts(&Cli::command(), &crate::render::GUIDE);
}

#[test]
fn every_command_matches_the_checked_in_command_reference() {
    let reference = concat!(env!("CARGO_MANIFEST_DIR"), "/docs/commands.md");
    crumbkit_test::check_command_reference(&super::help::binary(), reference);
}
