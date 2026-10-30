//! The plugins built into the binary, run in-process: plans, beside
//! installed plugins.

use super::plugins::{TASKS, installed};
use super::*;

/// Runs `board` in `directory` with the binary's built-in plugins and those
/// installed in `data`, if any, and returns what the run recorded.
fn built_in(directory: &Path, data: Option<&Scenario>, arguments: &[&str]) -> (Output, Ran) {
    let context = Context {
        start: directory.to_path_buf(),
        home: None,
        cache: None,
        plugins: data.map(|data| data.workdir().join("plugins")),
        boards: None,
        signer: None,
        allowed_signers: None,
        environment: board_plugin::Environment {
            path: Some("/usr/bin:/bin".into()),
            ..board_plugin::Environment::default()
        },
        complete: None,
        terminal: false,
        browser: None,
        binary: None,
        cursor: None,
        built_in: board_plugins::BUILT_IN,
    };
    let arguments = std::iter::once(BINARY_NAME)
        .chain(arguments.iter().copied())
        .map(OsString::from);
    let mut ran = Ran::default();
    let output = run(arguments, &context, &mut &b""[..], &mut ran);
    (output, ran)
}

/// `plugin list --json`'s row for `name`.
fn listed(board: &Scenario, data: &Scenario, name: &str) -> Value {
    let (output, _) = built_in(board.workdir(), Some(data), &["plugin", "list", "--json"]);
    let rows = json(&output)["data"].as_array().unwrap().clone();
    (rows.into_iter()).find(|row| row["name"] == name).unwrap()
}

#[test]
fn a_built_in_plugin_is_listed_without_a_program_to_run_and_an_installed_one_keeps_its_own() {
    let (board, data) = installed();
    assert_eq!(listed(&board, &data, "plans").get("run"), None);
    assert_eq!(listed(&board, &data, "tasks")["run"], json!(["./run"]));
}

#[test]
fn an_installed_copy_of_a_built_in_plugin_is_listed_with_its_directory_and_one_under_another_name_is_not()
 {
    for (name, shadowed) in [("plans", true), ("tusks", false)] {
        let (board, data) = installed();
        let manifest = TASKS.replace(r#""tasks""#, &format!("{name:?}"));
        data.write(
            &format!("plugins/{name}/plugin.json"),
            Item::File(&manifest),
        );
        let directory = data.workdir().join("plugins").join(name);
        let row = listed(&board, &data, "plans");
        let (output, _) = built_in(board.workdir(), Some(&data), &["plugin", "list"]);
        let line = (text(&output.stdout).lines())
            .find(|line| line.contains(" plans "))
            .unwrap()
            .to_owned();
        if shadowed {
            assert_eq!(row["code"], "board::plugin::duplicate");
            assert_eq!(row["shadows"], json!(directory.display().to_string()));
            let expected = format!(
                "degraded board::plugin::duplicate plans 0.1.0 disabled: installed again at {} offers open",
                directory.display()
            );
            assert_eq!(line, expected);
        } else {
            assert_eq!((row.get("code"), row.get("shadows")), (None, None));
            assert_eq!(line, "ok plans 0.1.0 disabled offers open");
        }
    }
}

/// A board that enables plans, holding an open question under
/// `projects/a` and another under `projects/b`.
fn questions() -> Scenario {
    let board = Scenario::new();
    let config = r#"{"$type": "board/config/v1", "commit_on_put": false}"#;
    board.write(".board/board.json", Item::File(config));
    let (output, _) = built_in(board.workdir(), None, &["plugin", "enable", "plans"]);
    assert_eq!(output.code, 0, "{}", text(&output.stderr));
    for project in ["a", "b"] {
        let question = json!({
            "$type": "plans/question/v1",
            "question": "Which way?",
            "session": "local_0a1b",
            "asked": "2026-10-01T09:00:00Z",
            "status": "open",
        });
        let path = format!("projects/{project}/questions/q.json");
        board.write(&path, Item::File(&question.to_string()));
    }
    board
}

#[test]
fn call_plans_open_reads_its_path_flag_from_the_current_directory_and_starts_no_process() {
    let board = questions();
    let projects = board.workdir().join("projects");
    for (path, expected) in [
        ("a", "/projects/a/questions/q"),
        ("b", "/projects/b/questions/q"),
    ] {
        let arguments = ["--json", "call", "plans.open", "--path", path];
        let (output, ran) = built_in(&projects, None, &arguments);
        assert_eq!(output.code, 0, "{}", text(&output.stdout));
        let questions = &json(&output)["data"]["output"]["questions"];
        assert_eq!(questions.as_array().unwrap().len(), 1, "{questions}");
        assert_eq!(questions[0]["path"], expected);
        assert_eq!(ran.trace.work.get("started"), Some(&0));
    }
}

#[test]
fn call_plans_open_help_lists_its_input_as_flags() {
    let board = questions();
    let (output, _) = built_in(board.workdir(), None, &["call", "plans.open", "-h"]);
    assert_eq!(output.code, 0, "{}", text(&output.stderr));
    let help = text(&output.stdout);
    for flag in [
        "--path",
        "--limit",
        "The questions under a path whose status is open",
    ] {
        assert!(help.contains(flag), "{flag}: {help}");
    }
}
