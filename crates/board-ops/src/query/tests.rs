//! Querying the index with SQL.

use std::path::Path;

use board_model::Code;
use board_scenario::{Item, Scenario};
use serde_json::{Value, json};

use crate::query::retried;
use crate::snapshot::Need;
use crate::tests::{board, open, task};
use crate::{Board, QueryReport};
use board_index::{Index, IndexError};

/// Rows as a query returns them.
type Rows = Vec<Vec<serde_json::Value>>;

#[test]
fn a_missing_table_is_refused_naming_the_tables_in_its_fields_and_a_real_one_is_read() {
    let scenario = board();
    let refused = open(&scenario)
        .query("SELECT * FROM entires", 10, None, None)
        .unwrap_err();
    assert_eq!(refused.code(), Code::QueryInvalid);
    let refusal = refused.refusal();
    let Some(Value::Array(tables)) = refusal.fields.get("tables") else {
        panic!("no tables: {:?}", refusal.fields);
    };
    assert!(tables.contains(&json!("entries")), "{tables:?}");
    assert_eq!(refusal.fields.len(), 1, "{:?}", refusal.fields);
    assert!(refusal.hint.is_some());
    assert_eq!(refusal.message, refused.to_string());
    let admitted = open(&scenario).query("SELECT * FROM entries", 10, None, None);
    assert!(admitted.is_ok());
}

#[test]
fn a_missing_column_is_refused_naming_each_tables_columns_in_its_fields_and_a_real_one_is_read() {
    let scenario = board();
    let refusal = open(&scenario)
        .query("SELECT logcal FROM entries", 10, None, None)
        .unwrap_err()
        .refusal();
    assert_eq!(refusal.code.as_str(), "board::query::invalid");
    let Some(Value::Array(columns)) = refusal.fields.get("columns").and_then(|c| c.get("entries"))
    else {
        panic!("no columns: {:?}", refusal.fields);
    };
    assert!(columns.contains(&json!("logical")), "{columns:?}");
    assert!(refusal.hint.is_some());
    let admitted = open(&scenario).query("SELECT logical FROM entries", 10, None, None);
    assert!(admitted.is_ok());
}

fn query(sql: &str, limit: usize) -> Result<QueryReport, Code> {
    let scenario = board();
    scenario
        .write("tasks/a.json", Item::File(&task("")))
        .write("tasks/b.json", Item::File(r#"{"$type": "lr/task/v1"}"#));
    open(&scenario)
        .query(sql, limit, None, None)
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
fn records_lists_each_record_with_its_status_and_degraded_ones_with_their_code() {
    let scenario = board();
    scenario
        .write("tasks/a.json", Item::File(&task("")))
        .write("tasks/b.json", Item::File(r#"{"$type": "lr/task/v1"}"#))
        .write("bad.json", Item::File(r#"{"$type": "Not A Type"}"#))
        .write("notes.json", Item::File("{}"));
    let report = open(&scenario)
        .query(
            "SELECT path, type, status, code, body IS NOT NULL AS parsed FROM records
             WHERE path <> '.board/board.json' ORDER BY path",
            500,
            None,
            None,
        )
        .unwrap();
    assert_eq!(
        report.rows,
        [
            vec![
                json!("bad.json"),
                json!(null),
                json!("degraded"),
                json!("board::record::bad_type"),
                json!(0)
            ],
            vec![
                json!("projects/p.json"),
                json!("lr/project/v1"),
                json!("ok"),
                json!(null),
                json!(1)
            ],
            vec![
                json!("tasks/a.json"),
                json!("lr/task/v1"),
                json!("ok"),
                json!(null),
                json!(1)
            ],
            vec![
                json!("tasks/b.json"),
                json!("lr/task/v1"),
                json!("degraded"),
                json!("board::record::invalid"),
                json!(1)
            ],
        ],
        "notes.json has no $type, so it is not a record"
    );
}

#[test]
fn a_record_body_is_its_canonical_form() {
    let scenario = board();
    scenario.write(
        "tasks/a.json",
        Item::File(r#"{"title":"t",   "$type":"lr/task/v1","see":"/projects/p"}"#),
    );
    let report = open(&scenario)
        .query(
            "SELECT body FROM records WHERE path = 'tasks/a.json'",
            500,
            None,
            None,
        )
        .unwrap();
    assert_eq!(
        report.rows,
        [vec![json!(
            "{\n  \"$type\": \"lr/task/v1\",\n  \"see\": \"/projects/p\",\n  \"title\": \"t\"\n}\n"
        )]]
    );
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
        "DELETE FROM main.blobs",
        "SELECT 1; SELECT 2",
    ] {
        assert_eq!(
            query(refused, 500).unwrap_err(),
            Code::QueryInvalid,
            "{refused}"
        );
    }
}

/// Queries a board through an index file, and between deciding its views
/// and running the statement, each time, checks a changed task through a
/// second handle on the file and then, the first `prunes` times, prunes.
/// Returns the rows queried or the error's code and message, how many times
/// the statement was reached, and the rows a cold index gives before and
/// after the changes.
fn queried_while_another_handle_prunes(
    prunes: usize,
) -> (Result<Rows, (Code, String)>, usize, [Rows; 2]) {
    let scenario = board();
    scenario.write("tasks/a.json", Item::File(&task("")));
    let root = scenario.workdir().to_path_buf();
    let sql = "SELECT path, blob, type, status FROM entries ORDER BY path";
    let cold = || {
        let board = Board::open(&root, None, "").unwrap();
        board.query(sql, 100, None, None).unwrap().rows
    };
    let before = cold();
    let cache = tempfile::tempdir().unwrap();
    let board = Board::open(&root, None, "").unwrap();
    let mut runs = 0;
    let warm = retried(|| {
        let (view, memory) = board.decided(Some(cache.path()))?;
        runs += 1;
        if runs <= prunes.max(1) {
            let title = format!(r#"{{"$type": "lr/task/v1", "title": "u{runs}"}}"#);
            scenario.write("tasks/a.json", Item::File(&title));
            let other = open(&scenario);
            let second = || Index::open(Some(cache.path()), other.repository().path()).unwrap();
            other.snapshot(second(), None, Need::Report).unwrap();
            if runs <= prunes {
                let writer = second().write().unwrap();
                writer.prune().unwrap();
                writer.decide(Vec::new()).unwrap();
            }
        }
        board.run(&view, memory, sql, 100, None)
    })
    .map(|report| report.rows)
    .map_err(|error| (error.code(), error.to_string()));
    (warm, runs, [before, cold()])
}

#[test]
fn a_query_another_process_prunes_under_runs_again_and_answers_as_a_cold_index() {
    let (warm, runs, [before, after]) = queried_while_another_handle_prunes(1);
    assert_ne!(before, after);
    assert_eq!(warm.unwrap(), after);
    assert_eq!(runs, 2);
}

#[test]
fn a_query_another_process_writes_under_without_pruning_answers_from_its_own_run() {
    let (warm, runs, [before, after]) = queried_while_another_handle_prunes(0);
    assert_ne!(before, after);
    assert_eq!(warm.unwrap(), before);
    assert_eq!(runs, 1);
}

#[test]
fn a_query_pruned_under_three_times_is_refused_as_pruned() {
    let (warm, runs, _) = queried_while_another_handle_prunes(3);
    assert_eq!(
        warm.unwrap_err(),
        (Code::IndexFailed, IndexError::Pruned.to_string())
    );
    assert_eq!(runs, 3);
}

#[test]
fn a_query_pruned_under_twice_answers_on_its_third_run() {
    let (warm, runs, [_, after]) = queried_while_another_handle_prunes(2);
    assert_eq!(warm.unwrap(), after);
    assert_eq!(runs, 3);
}

/// What `sql` answers through the index under `cache`, less whether that
/// index was in memory.
fn answered(scenario: &Scenario, sql: &str, cache: Option<&Path>) -> QueryReport {
    QueryReport {
        memory: None,
        ..open(scenario).query(sql, 10_000, None, cache).unwrap()
    }
}

/// The views answer from a warm index as from a cold one.
#[test]
fn a_query_answers_from_a_warm_index_as_from_a_cold_one_after_a_change() {
    let scenario = board();
    scenario.write("tasks/a.json", Item::File(&task("")));
    let library = crate::tests::Library::mounted(&scenario);
    let cache = tempfile::tempdir().unwrap();
    answered(&scenario, "SELECT 1", Some(cache.path()));
    library.moved(&scenario);
    scenario
        .write("tasks/a.json", Item::File(&task(r#", "see": "/nowhere""#)))
        .write("tasks/b.json", Item::File(&task("")));
    for sql in [
        "SELECT * FROM entries ORDER BY source, path",
        "SELECT * FROM records ORDER BY source, path",
        "SELECT * FROM refs ORDER BY record, schema, ordinal",
        "SELECT * FROM links ORDER BY blob, ordinal",
    ] {
        let warm = answered(&scenario, sql, Some(cache.path()));
        assert_eq!(warm, answered(&scenario, sql, None), "{sql}");
    }
}
