//! `board <alias> <capability>` runs what `board call <plugin>.<capability>`
//! runs (docs/plugins.md, Aliases).

use super::plugins::RESULT;
use super::*;

/// A manifest of `name` with the alias `alias`, whose capability `due`
/// takes the flag `--before`.
fn manifest(name: &str, alias: &str) -> String {
    json!({
        "$type": "board/plugin-manifest/v1",
        "name": name,
        "version": "0.1.0",
        "description": "Open tasks.",
        "alias": alias,
        "run": ["./run"],
        "capabilities": [{
            "name": "due",
            "description": "Open tasks due before a date.",
            "inputSchema": {"type": "object", "properties": {
                "before": {"type": "string"}
            }, "additionalProperties": false},
            "outputSchema": {"type": "object"}
        }]
    })
    .to_string()
}

/// A board enabling `enabled`, and a plugins directory holding tasks with
/// the alias `alias`. The plugin saves its request and `BOARD_PLUGIN`
/// beside its manifest.
fn aliased(enabled: &[&str], alias: &str) -> (Scenario, Scenario) {
    let board = Scenario::new();
    let config = json!({"$type": "board/config/v1", "plugins": enabled});
    board.write(".board/board.json", Item::File(&config.to_string()));
    let data = Scenario::new();
    install(&data, "tasks", alias);
    (board, data)
}

fn install(data: &Scenario, name: &str, alias: &str) {
    let directory = data.workdir().join("plugins").join(name);
    let script = format!(
        "#!/bin/sh\ncat > '{0}/request'\nprintf '%s' \"$BOARD_PLUGIN\" > '{0}/plugin'\nprintf '%s' '{RESULT}'\n",
        directory.display()
    );
    data.write(
        &format!("plugins/{name}/plugin.json"),
        Item::File(&manifest(name, alias)),
    )
    .write(&format!("plugins/{name}/run"), Item::Executable(&script));
}

fn run_in(board: &Scenario, data: &Scenario, arguments: &[&str]) -> Output {
    let plugins = data.workdir().join("plugins");
    board_full(board.workdir(), None, Some(&plugins), arguments, b"")
}

/// The request the plugin was sent and the `BOARD_PLUGIN` it saw, which
/// the read removes, so the next call writes them afresh.
fn taken(data: &Scenario) -> Option<(Value, String)> {
    let directory = data.workdir().join("plugins/tasks");
    let request = std::fs::read(directory.join("request")).ok()?;
    let plugin = std::fs::read_to_string(directory.join("plugin")).ok()?;
    data.write("plugins/tasks/request", Item::Deleted);
    Some((serde_json::from_slice(&request).unwrap(), plugin))
}

fn code(output: &Output) -> Value {
    json(output)["code"].clone()
}

#[test]
fn an_alias_runs_what_board_call_runs_with_the_same_input_output_and_exit_code() {
    let (board, data) = aliased(&["tasks"], "tk");
    let lines: [(&[&str], &[&str]); 3] = [
        (
            &["tk", "due", "--before", "2026-10-05"],
            &["call", "tasks.due", "--before", "2026-10-05"],
        ),
        (
            &["--json", "tk", "due", "--before", "x"],
            &["--json", "call", "tasks.due", "--before", "x"],
        ),
        (
            &["tk", "due", "--after", "x"],
            &["call", "tasks.due", "--after", "x"],
        ),
    ];
    for (alias, call) in lines {
        let through_alias = run_in(&board, &data, alias);
        let sent_through_alias = taken(&data);
        let through_call = run_in(&board, &data, call);
        assert_eq!(
            text(&through_alias.stdout),
            text(&through_call.stdout),
            "{alias:?}"
        );
        assert_eq!(
            text(&through_alias.stderr),
            text(&through_call.stderr),
            "{alias:?}"
        );
        assert_eq!(through_alias.code, through_call.code, "{alias:?}");
        assert_eq!(sent_through_alias, taken(&data), "{alias:?}");
    }
    let output = run_in(&board, &data, &["tk", "due", "--before", "2026-10-05"]);
    assert_eq!(output.code, 0, "{}", text(&output.stderr));
    let (request, plugin) = taken(&data).unwrap();
    assert_eq!(request["input"], json!({"before": "2026-10-05"}));
    assert_eq!(plugin, "tasks");
}

#[test]
fn an_alias_of_a_plugin_the_board_does_not_enable_is_refused_and_one_it_enables_runs() {
    for (enabled, expected) in [(&[][..], 2), (&["tasks"][..], 0)] {
        let (board, data) = aliased(enabled, "tk");
        let output = run_in(&board, &data, &["--json", "tk", "due"]);
        assert_eq!(output.code, expected, "{enabled:?}");
        if expected == 2 {
            assert_eq!(code(&output), "board::cli::usage");
            assert_eq!(taken(&data), None);
        }
    }
}

#[test]
fn a_built_in_command_wins_over_an_alias_of_the_same_word() {
    for (alias, word) in [("ls", "ls"), ("tk", "tk")] {
        let (board, data) = aliased(&["tasks"], alias);
        let output = run_in(&board, &data, &[word, "due"]);
        assert_eq!(taken(&data).is_some(), alias == "tk", "{alias}");
        if alias == "ls" {
            assert_eq!(output.code, 1, "ls of /due finds nothing");
        }
    }
}

#[test]
fn an_alias_two_enabled_plugins_share_is_refused_and_board_call_still_runs() {
    let (board, data) = aliased(&["tasks", "todo"], "tk");
    install(&data, "todo", "tk");
    let output = run_in(&board, &data, &["--json", "tk", "due"]);
    assert_eq!(output.code, 1);
    assert_eq!(code(&output), "board::plugin::alias_conflict");
    assert_eq!(
        json(&output)["message"],
        "the tasks plugin's alias tk is also the alias of the todo plugin, which the board enables"
    );
    assert_eq!(taken(&data), None);
    let twin = run_in(&board, &data, &["--json", "call", "tasks.due"]);
    assert_eq!(twin.code, 0, "{}", text(&twin.stderr));
}

#[test]
fn help_lists_an_enabled_plugins_alias_and_the_aliases_help_lists_its_capabilities() {
    let line = "Run a capability of the tasks plugin: `board tk <capability>` is `board call tasks.<capability>`.";
    for (enabled, listed) in [(&[][..], false), (&["tasks"][..], true)] {
        let (board, data) = aliased(enabled, "tk");
        let output = run_in(&board, &data, &["-h"]);
        assert_eq!(output.code, 0);
        assert_eq!(text(&output.stdout).contains(line), listed, "{enabled:?}");
    }
    let (board, data) = aliased(&["tasks"], "tk");
    let output = run_in(&board, &data, &["tk", "-h"]);
    assert_eq!(output.code, 0, "{}", text(&output.stderr));
    let help = text(&output.stdout);
    assert!(
        help.contains("due  Open tasks due before a date."),
        "{help}"
    );
}

#[test]
fn enable_refuses_an_alias_that_is_a_built_in_command_and_admits_another_word() {
    for (alias, refused) in [("get", true), ("tk", false)] {
        let (board, data) = aliased(&[], alias);
        board.write(
            ".git/config",
            Item::Appended("[user]\n\tname = T\n\temail = t@example.com\n"),
        );
        let output = run_in(&board, &data, &["--json", "plugin", "enable", "tasks"]);
        if refused {
            assert_eq!(output.code, 1);
            assert_eq!(code(&output), "board::plugin::alias_conflict");
            assert_eq!(
                json(&output)["message"],
                "the tasks plugin's alias get is also Board's built-in command get"
            );
        } else {
            assert_eq!(output.code, 0, "{}", text(&output.stdout));
        }
    }
}
