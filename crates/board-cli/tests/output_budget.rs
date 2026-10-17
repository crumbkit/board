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
    let home = tempfile::tempdir().unwrap();
    Command::new(env!("CARGO_BIN_EXE_board"))
        .args(["--json", "ls", "/"])
        .current_dir(scenario.workdir())
        .env_clear()
        .env("HOME", home.path())
        .env("XDG_CACHE_HOME", home.path())
        .env(variable, "10")
        .output()
        .unwrap()
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
