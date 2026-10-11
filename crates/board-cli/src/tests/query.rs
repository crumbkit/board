//! The byte budget of a `board query` run.

use board_model::Code;
use board_ops::{Board, Plugins, QueryError, QueryReport};
use board_scenario::Scenario;

use super::{board, text};
use crate::render::{self, Format};
use crate::report::Output;

/// Three rows of one column, `["a"]`, `["b"]` and `["c"]`.
const SQL: &str = "SELECT * FROM (VALUES ('a'), ('b'), ('c'))";

/// `sql` answered from `scenario` within `limit` rows and `budget` bytes of
/// rows, as `format` prints them.
fn answered(
    scenario: &Scenario,
    sql: &str,
    format: Format,
    limit: usize,
    budget: usize,
) -> Result<QueryReport, QueryError> {
    let board = Board::open(scenario.workdir(), None).unwrap();
    let budget = Some(render::rows_budget(format, budget));
    board.query(sql, limit, budget, None, &Plugins::default())
}

/// [`SQL`] answered as [`answered`] answers it, and rendered as `board query`
/// renders it.
fn rendered(scenario: &Scenario, format: Format, limit: usize, budget: usize) -> Output {
    render::query(
        format,
        &answered(scenario, SQL, format, limit, budget).unwrap(),
    )
}

/// The note every run here starts its stderr with, as it has no cache.
const MEMORY: &str = "note: the index is in memory for this run: there is no cache directory\n";

#[test]
fn query_prints_each_row_within_its_budget_and_notes_the_rows_left_at_it() {
    let scenario = board();
    let ran = |budget| {
        let ran = rendered(&scenario, Format::Text, 10, budget);
        (ran.code, text(&ran.stdout), text(&ran.stderr))
    };
    // Each row prints as a line of 6 bytes, such as `["a"]` and its newline.
    let all = "[\"column1\"]\n[\"a\"]\n[\"b\"]\n[\"c\"]\n";
    assert_eq!(
        ran(18),
        (0, all.to_owned(), MEMORY.to_owned()),
        "18 bytes fit 3 rows"
    );
    let note = format!(
        "{MEMORY}note: showing the first 2 rows, and more matched, but the next would take \
         the output past its budget (board::query::over_budget); {}\n",
        Code::QueryOverBudget.hint(&board_model::Fields::default())
    );
    let two = "[\"column1\"]\n[\"a\"]\n[\"b\"]\n";
    assert_eq!(ran(17), (0, two.to_owned(), note), "17 bytes fit 2");
    let first = ran(6);
    assert_eq!(first.1, "[\"column1\"]\n[\"a\"]\n", "6 bytes fit 1");
    assert!(first.2.contains("showing the first 1 rows"), "{}", first.2);
    let refused = answered(&scenario, SQL, Format::Text, 10, 5).unwrap_err();
    assert_eq!(refused.code(), Code::QueryOverBudget, "5 bytes fit none");
}

#[test]
fn a_query_report_names_its_code_when_the_budget_left_rows_out_and_not_when_the_limit_did() {
    let scenario = board();
    let report = |limit, budget| super::json(&rendered(&scenario, Format::Json, limit, budget));
    // Each row prints in `rows` as 23 bytes, such as `,\n    [\n      "a"\n    ]`,
    // and the first row's missing comma and the closing `\n  ]` add 2.
    let over = report(10, 70);
    assert_eq!(over["rows"], serde_json::json!([["a"], ["b"]]));
    assert_eq!(over["truncated"], true);
    assert_eq!(over["code"], "board::query::over_budget");
    let whole = report(10, 71);
    assert_eq!(whole["rows"].as_array().unwrap().len(), 3);
    assert_eq!(whole["truncated"], false);
    assert!(whole.get("code").is_none(), "{whole}");
    // The twin cut by the limit is truncated with no code.
    let limited = report(2, 71);
    assert_eq!(limited["truncated"], true);
    assert!(limited.get("code").is_none(), "{limited}");
}

#[test]
fn the_query_budget_counts_every_byte_its_rows_add_to_the_printed_output() {
    let scenario = board();
    for format in [Format::Text, Format::Json] {
        let printed = |rows: usize| {
            let sql = format!("{SQL} LIMIT {rows}");
            let report = answered(&scenario, &sql, format, 10, usize::MAX).unwrap();
            (render::query(format, &report).stdout.len(), report.rows)
        };
        let budget = render::rows_budget(format, 1000);
        let framing = 1000 - budget.bytes;
        let (empty, _) = printed(0);
        for count in [1, 3] {
            let (bytes, rows) = printed(count);
            let counted: usize = rows
                .iter()
                .map(|row| budget.row + row.iter().map(budget.value).sum::<usize>())
                .sum();
            assert_eq!(bytes - empty, framing + counted, "{format:?} {count}");
        }
    }
}

#[test]
fn a_query_refuses_a_value_longer_than_its_budget_and_admits_one_as_long() {
    let scenario = board();
    // `zeroblob(40)` is 40 bytes long, though it prints as `[40]`. SQLite
    // limits no value to less than 30 bytes, so a smaller value could not
    // show the limit.
    let sql = "SELECT length(zeroblob(40))";
    let refused = answered(&scenario, sql, Format::Text, 10, 39).unwrap_err();
    assert_eq!(refused.code(), Code::QueryOverBudget);
    let admitted = answered(&scenario, sql, Format::Text, 10, 40).unwrap();
    assert_eq!(admitted.rows, [vec![serde_json::json!(40)]]);
}

#[test]
fn a_query_refuses_a_first_row_that_prints_past_its_budget_and_admits_one_that_fits() {
    let scenario = board();
    // Each value is 30 bytes, within the budget, and prints as 60 hex
    // digits in quotes, so the row prints as 128 bytes with `[`, `,`, `]`
    // and the newline.
    let sql = "SELECT zeroblob(30), zeroblob(30)";
    let refused = answered(&scenario, sql, Format::Text, 10, 127).unwrap_err();
    assert_eq!(refused.code(), Code::QueryOverBudget);
    let admitted = answered(&scenario, sql, Format::Text, 10, 128).unwrap();
    let hex = serde_json::json!("00".repeat(30));
    assert_eq!(admitted.rows, [vec![hex.clone(), hex]]);
}

/// `board query --json` of [`SQL`] when the budget leaves its last row out.
const GOLDEN: &str = r#"{
  "$type": "board/query-report/v1",
  "code": "board::query::over_budget",
  "columns": [
    "column1"
  ],
  "plugins": [],
  "rows": [
    [
      "a"
    ],
    [
      "b"
    ]
  ],
  "truncated": true
}
"#;

#[test]
fn a_query_report_over_its_budget_matches_its_golden_bytes() {
    let ran = rendered(&board(), Format::Json, 10, 70);
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
            report["rows"].as_array().unwrap().len(),
            report["truncated"].clone(),
        )
    };
    assert_eq!(rows(&["--limit", "2"]), (2, serde_json::json!(true)));
    assert_eq!(rows(&["--all"]), (3, serde_json::json!(false)));
}

#[test]
fn query_all_is_still_cut_at_its_budget_with_the_code_and_not_within_it() {
    let scenario = board();
    // `--all` asks for every row, as `usize::MAX` does here.
    let report = |budget| super::json(&rendered(&scenario, Format::Json, usize::MAX, budget));
    let over = report(70);
    assert_eq!(
        (over["rows"].as_array().unwrap().len(), &over["truncated"]),
        (2, &serde_json::json!(true))
    );
    assert_eq!(over["code"], "board::query::over_budget");
    let whole = report(71);
    assert_eq!(
        (whole["rows"].as_array().unwrap().len(), &whole["truncated"]),
        (3, &serde_json::json!(false))
    );
    assert!(whole.get("code").is_none(), "{whole}");
}
