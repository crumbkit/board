//! Querying the index with SQL.

use board_model::Code;
use board_scenario::Item;
use serde_json::json;

use crate::QueryReport;
use crate::tests::{board, open, task};

fn query(sql: &str, limit: usize) -> Result<QueryReport, Code> {
    let scenario = board();
    scenario
        .write("tasks/a.json", Item::File(&task("")))
        .write("tasks/b.json", Item::File(r#"{"$type": "lr/task/v1"}"#));
    open(&scenario)
        .query(sql, limit, None, &crate::Plugins::default())
        .map_err(|error| error.code())
}

#[test]
fn a_query_reads_the_entries_check_decided() {
    let report = query(
        "SELECT path, status, code FROM entries WHERE type = 'lr/task/v1' ORDER BY path",
        500,
    )
    .unwrap();
    assert_eq!(report.columns, ["path", "status", "code"]);
    assert_eq!(
        report.rows,
        [
            vec![json!("tasks/a.json"), json!("ok"), json!(null)],
            vec![
                json!("tasks/b.json"),
                json!("degraded"),
                json!("board::record::invalid")
            ],
        ]
    );
    assert!(!report.truncated);
}

#[test]
fn a_query_returns_at_most_its_limit_and_says_when_rows_were_left_out() {
    let all = query("SELECT path FROM entries", 500).unwrap();
    let limited = query("SELECT path FROM entries", 2).unwrap();
    assert!(all.rows.len() > 2);
    assert_eq!(limited.rows.len(), 2);
    assert!(limited.truncated);
    let exact = query("SELECT path FROM entries", all.rows.len()).unwrap();
    assert!(!exact.truncated, "exactly the limit leaves nothing out");
}

#[test]
fn sql_that_cannot_run_is_refused_and_one_read_is_admitted() {
    assert!(query("SELECT count(*) FROM blobs", 500).is_ok());
    for refused in [
        "SELECT nothing FROM nowhere",
        "SELEC 1",
        "DELETE FROM blobs",
        "SELECT 1; SELECT 2",
    ] {
        assert_eq!(
            query(refused, 500).unwrap_err(),
            Code::QueryInvalid,
            "{refused}"
        );
    }
}
