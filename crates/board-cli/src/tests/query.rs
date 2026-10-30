//! The byte budget of a `board query` run.

use board_model::Code;
use board_ops::{Board, QueryError, QueryReport};
use board_scenario::Scenario;
use serde_json::json;

use super::{board, text};
use crate::render::{self, Printer};
use crate::report::Output;

/// Three rows of one column, `["a"]`, `["b"]` and `["c"]`.
const SQL: &str = "SELECT * FROM (VALUES ('a'), ('b'), ('c'))";

/// `sql` answered from `scenario` within `limit` rows and `budget` bytes of
/// rows.
fn answered(
    scenario: &Scenario,
    sql: &str,
    limit: usize,
    budget: usize,
) -> Result<QueryReport, QueryError> {
    let board = Board::open(scenario.workdir(), None).unwrap();
    let budget = Some(render::rows_budget(budget));
    board.query(sql, limit, budget, None)
}

/// [`SQL`] answered as [`answered`] answers it, and rendered as `board query`
/// renders it.
fn rendered(scenario: &Scenario, format: bool, limit: usize, budget: usize) -> Output {
    let report = answered(scenario, SQL, limit, budget).unwrap();
    render::query(Printer::of(format, 1 << 20, &[]), &report)
}

#[test]
fn query_prints_each_row_within_its_budget_and_notes_the_rows_left_at_it() {
    let scenario = board();
    let ran = |budget| {
        let ran = rendered(&scenario, false, 10, budget);
        (ran.code, text(&ran.stdout), text(&ran.stderr))
    };
    // Each row counts as 7 bytes: `["a"]`, the comma or line feed after it,
    // and a comma after its value.
    let all = "[\"column1\"]\n[\"a\"]\n[\"b\"]\n[\"c\"]\n";
    let three = ran(21);
    assert_eq!((three.0, three.1.as_str()), (0, all), "21 bytes fit 3 rows");
    assert!(!three.2.contains("over_budget"), "{}", three.2);
    let two = ran(20);
    assert_eq!(two.1, "[\"column1\"]\n[\"a\"]\n[\"b\"]\n", "20 bytes fit 2");
    assert!(two.2.contains("(board::output::over_budget)"), "{}", two.2);
    let refused = answered(&scenario, SQL, 10, 6).unwrap_err();
    assert_eq!(refused.code(), Code::OutputOverBudget, "6 bytes fit none");
    assert!(answered(&scenario, SQL, 10, 7).is_ok(), "7 bytes fit one");
}

#[test]
fn a_query_report_names_its_code_when_the_budget_left_rows_out_and_not_when_the_limit_did() {
    let scenario = board();
    let report = |limit, budget| super::json(&rendered(&scenario, true, limit, budget));
    let over = report(10, 20);
    assert_eq!(over["data"], json!([["a"], ["b"]]));
    assert_eq!(over["truncated"], true);
    assert_eq!(over["summary"]["code"], "board::output::over_budget");
    let whole = report(10, 21);
    assert_eq!(whole["data"].as_array().unwrap().len(), 3);
    assert_eq!(whole["truncated"], false);
    assert!(whole["summary"].get("code").is_none(), "{whole}");
    // The twin cut by the limit is truncated with no code.
    let limited = report(2, 21);
    assert_eq!(limited["truncated"], true);
    assert!(limited["summary"].get("code").is_none(), "{limited}");
}

#[test]
fn a_query_refuses_a_value_longer_than_its_budget_and_admits_one_as_long() {
    let scenario = board();
    // `zeroblob(40)` is 40 bytes long, though it prints as `[40]`. SQLite
    // limits no value to less than 30 bytes, so a smaller value could not
    // show the limit.
    let sql = "SELECT length(zeroblob(40))";
    let refused = answered(&scenario, sql, 10, 39).unwrap_err();
    assert_eq!(refused.code(), Code::OutputOverBudget);
    let admitted = answered(&scenario, sql, 10, 40).unwrap();
    assert_eq!(admitted.rows, [vec![json!(40)]]);
}

#[test]
fn a_query_refuses_a_first_row_that_prints_past_its_budget_and_admits_one_that_fits() {
    let scenario = board();
    // Each value is 30 bytes, within the budget, and prints as 60 hex
    // digits in quotes and a comma, so the row counts as 129 bytes.
    let sql = "SELECT zeroblob(30), zeroblob(30)";
    let refused = answered(&scenario, sql, 10, 128).unwrap_err();
    assert_eq!(refused.code(), Code::OutputOverBudget);
    let admitted = answered(&scenario, sql, 10, 129).unwrap();
    let hex = json!("00".repeat(30));
    assert_eq!(admitted.rows, [vec![hex.clone(), hex]]);
}

/// `board query --json` of [`SQL`] when the budget leaves its last row out.
const GOLDEN: &str = concat!(
    r#"{"schema":"board/query-report/v2","data":[["a"],["b"]],"truncated":true,"#,
    r#""summary":{"code":"board::output::over_budget","columns":["column1"]}}"#,
    "\n"
);

#[test]
fn a_query_report_over_its_budget_matches_its_golden_bytes() {
    let ran = rendered(&board(), true, 10, 20);
    assert_eq!((ran.code, text(&ran.stdout)), (0, GOLDEN.to_owned()));
}

#[test]
fn query_all_returns_every_row_that_a_limit_leaves_out() {
    let scenario = board();
    let rows = |page: &[&str]| {
        let ran = super::board_in(
            scenario.workdir(),
            &[&["--json", "query", SQL], page].concat(),
        );
        let report = super::json(&ran);
        (
            report["data"].as_array().unwrap().len(),
            report["truncated"].clone(),
        )
    };
    assert_eq!(rows(&["--limit", "2"]), (2, json!(true)));
    assert_eq!(rows(&["--all"]), (3, json!(false)));
}

#[test]
fn query_all_is_still_cut_at_its_budget_with_the_code_and_not_within_it() {
    let scenario = board();
    // `--all` asks for every row, as `usize::MAX` does here.
    let report = |budget| super::json(&rendered(&scenario, true, usize::MAX, budget));
    let over = report(20);
    assert_eq!(
        (over["data"].as_array().unwrap().len(), &over["truncated"]),
        (2, &json!(true))
    );
    assert_eq!(over["summary"]["code"], "board::output::over_budget");
    let whole = report(21);
    assert_eq!(
        (whole["data"].as_array().unwrap().len(), &whole["truncated"]),
        (3, &json!(false))
    );
    assert!(whole["summary"].get("code").is_none(), "{whole}");
}
