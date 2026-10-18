//! `BOARD_OUTPUT_BUDGET` lowers the bytes one run may print, and a result
//! past it fails with `board::output::over_budget` (docs/architecture/errors.md).
//! crumbkit reads the variable from the process environment, so only a child
//! process can be given it.
#![cfg(test)]
#![expect(
    clippy::disallowed_methods,
    reason = "only a child process can be given its own output budget variable"
)]

use std::process::{Command, Output};

use board_scenario::{Item, Scenario};

fn ls_json(scenario: &Scenario, variable: &str) -> Output {
    run(scenario, &["--json", "ls", "/"], Some((variable, "10")))
}

/// Runs `board` with `arguments` in `scenario`, given `variable` if one.
fn run(scenario: &Scenario, arguments: &[&str], variable: Option<(&str, &str)>) -> Output {
    let home = tempfile::tempdir().unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_board"));
    command
        .args(arguments)
        .current_dir(scenario.workdir())
        .env_clear()
        .env("HOME", home.path())
        .env("XDG_CACHE_HOME", home.path());
    command.envs(variable);
    command.output().unwrap()
}

#[test]
fn board_output_budget_refuses_a_larger_result_and_the_old_crumbkit_variable_does_not() {
    let scenario = Scenario::new();
    scenario.write(
        ".board/board.json",
        Item::File(r#"{"$type": "board/config/v1"}"#),
    );
    let refused = ls_json(&scenario, "BOARD_OUTPUT_BUDGET");
    assert_eq!(refused.status.code(), Some(1));
    let error: serde_json::Value = serde_json::from_slice(&refused.stdout).unwrap();
    assert_eq!(error["schema"], "board/failure/v1");
    assert_eq!(error["code"], "board::output::over_budget");
    assert_eq!(error["fields"]["budget"], 10);
    let admitted = ls_json(&scenario, "CRUMBKIT_OUTPUT_BUDGET");
    assert_eq!(admitted.status.code(), Some(0));
    let report: serde_json::Value = serde_json::from_slice(&admitted.stdout).unwrap();
    assert_eq!(report["schema"], "board/ls-report/v2");
}

/// A board of five entries: its config and four records with long names,
/// so that a small budget holds some of their paths and not all.
fn five_entries() -> Scenario {
    let scenario = Scenario::new();
    scenario.write(
        ".board/board.json",
        Item::File(r#"{"$type": "board/config/v1"}"#),
    );
    for name in ["a", "b", "c", "d"] {
        scenario.write(
            &format!("{name}-record-with-a-long-name.json"),
            Item::File("{}"),
        );
    }
    scenario
}

fn json(output: &Output) -> serde_json::Value {
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn a_query_cut_by_the_budget_prints_no_total_and_neither_does_one_within_it() {
    let scenario = five_entries();
    let query = ["--json", "query", "select path from entries"];
    let cut = json(&run(
        &scenario,
        &query,
        Some(("BOARD_OUTPUT_BUDGET", "200")),
    ));
    assert_eq!(cut["data"], serde_json::json!([[".board/board.json"]]));
    assert_eq!(cut["truncated"], true);
    assert_eq!(cut["cut"]["code"], "board::output::over_budget");
    assert!(cut.get("total").is_none(), "{cut}");
    let whole = json(&run(&scenario, &query, None));
    assert_eq!(whole["data"].as_array().unwrap().len(), 5);
    assert_eq!(whole["truncated"], false);
    assert!(whole.get("total").is_none(), "{whole}");
}

#[test]
fn ls_cut_by_the_budget_keeps_its_total() {
    // The listing names the repository's path, so the budget leaves room
    // for a long temporary directory and still cuts the five entries.
    let ls = ["--json", "ls", "/"];
    let cut = json(&run(
        &five_entries(),
        &ls,
        Some(("BOARD_OUTPUT_BUDGET", "400")),
    ));
    assert_eq!(cut["truncated"], true);
    assert_eq!(cut["cut"]["code"], "board::output::over_budget");
    assert_eq!(cut["total"], 5);
}
