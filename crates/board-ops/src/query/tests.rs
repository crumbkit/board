//! Querying the index with SQL.

use std::path::Path;

use board_model::Code;
use board_scenario::{Item, Scenario};
use proptest::prelude::*;
use serde_json::json;

use crate::tests::generated::{Generated, generated};
use crate::tests::{board, open, task};
use crate::{Board, Plugins, QueryReport};
use board_index::{Index, IndexError};

/// Rows as a query returns them.
type Rows = Vec<Vec<serde_json::Value>>;

/// The names a refusal of `sql` lists, or `None` when `sql` is read.
fn known(sql: &str) -> Option<crate::Known> {
    let scenario = board();
    match open(&scenario).query(sql, 10, None, &Plugins::default()) {
        Err(crate::QueryError::Unknown { known, .. }) => Some(known),
        Err(error) => panic!("{error}"),
        Ok(_) => None,
    }
}

#[test]
fn a_missing_table_is_refused_with_the_tables_there_are_and_a_real_one_is_read() {
    let Some(crate::Known::Tables(tables)) = known("SELECT * FROM entires") else {
        panic!("no tables");
    };
    assert!(tables.iter().any(|table| table == "entries"), "{tables:?}");
    assert!(tables.iter().any(|table| table == "records"), "{tables:?}");
    assert_eq!(known("SELECT * FROM entries"), None);
}

#[test]
fn a_missing_column_is_refused_with_the_columns_of_the_tables_named_and_a_real_one_is_read() {
    let Some(crate::Known::Columns(columns)) = known("SELECT logcal FROM Entries") else {
        panic!("no columns");
    };
    assert_eq!(columns.len(), 1, "{columns:?}");
    assert_eq!(columns[0].0, "entries");
    assert!(columns[0].1.iter().any(|column| column == "logical"));
    assert_eq!(known("SELECT logical FROM entries"), None);
}

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
            &Plugins::default(),
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
            &Plugins::default(),
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

/// A plugins directory holding `tasks`, which extracts `lr/task/v1` by
/// naming every blob it was sent.
fn tasks_plugin() -> Scenario {
    let plugins = Scenario::new();
    plugins
        .write(
            "tasks/plugin.json",
            Item::File(
                &json!({
                    "$type": "board/plugin-manifest/v1",
                    "name": "tasks",
                    "version": "0.1.0",
                    "description": "",
                    "run": ["./run"],
                    "extract": ["lr/task/v1"],
                    "capabilities": []
                })
                .to_string(),
            ),
        )
        .write(
            "tasks/run",
            Item::Executable(
                r#"#!/bin/sh
blobs=$(sed -n 's/^ *"blob": "\([0-9a-f]*\)",*$/"\1"/p' | paste -s -d, -)
printf '{"$type": "board/extract-result/v1", "blobs": [%s]}' "$blobs"
"#,
            ),
        );
    plugins
}

#[test]
fn a_plugin_shipping_an_unusable_schema_fails_the_query_as_a_bad_manifest_and_a_usable_one_does_not()
 {
    let board = Scenario::new();
    board.write(
        ".board/board.json",
        Item::File(r#"{"$type": "board/config/v1", "plugins": ["tasks"]}"#),
    );
    let installed = tasks_plugin();
    let manifest = std::fs::read(installed.workdir().join("tasks/plugin.json")).unwrap();
    let mut manifest: serde_json::Value = serde_json::from_slice(&manifest).unwrap();
    manifest["types"] = json!(["tasks/note/v1"]);
    installed
        .write("tasks/plugin.json", Item::File(&manifest.to_string()))
        .write(
            "tasks/types/tasks/note/v1.schema.json",
            Item::File(r#"{"type": 3}"#),
        );
    let plugins = Plugins {
        directory: Some(installed.workdir()),
        ..Plugins::default()
    };
    let report = open(&board).query("SELECT 1", 10, None, &plugins).unwrap();
    let codes: Vec<&str> = report
        .plugins
        .iter()
        .map(|failed| failed.code.as_str())
        .collect();
    assert_eq!(codes, ["board::plugin::bad_manifest"]);
    installed.write(
        "tasks/types/tasks/note/v1.schema.json",
        Item::File(r#"{"type": "object"}"#),
    );
    let report = open(&board).query("SELECT 1", 10, None, &plugins).unwrap();
    assert!(report.plugins.is_empty(), "{:?}", report.plugins);
}

/// Writes an empty file at `path`.
#[expect(
    clippy::disallowed_methods,
    reason = "a test signals a plugin it holds waiting"
)]
fn signal(path: &Path) {
    std::fs::write(path, b"").unwrap();
}

/// Signals the file it holds when dropped, so a test that fails part way
/// never leaves its plugin waiting.
struct Resume<'a>(&'a Path);

impl Drop for Resume<'_> {
    fn drop(&mut self) {
        signal(self.0);
    }
}

/// Queries a board through an index file while its plugin, each time it
/// extracts, waits for a second handle on the file to check a changed task
/// and then, the first `prunes` times, to prune. Returns the rows queried or
/// the error's code and message, how many times the plugin extracted, and the rows a cold index
/// gives before and after the changes.
fn queried_while_another_handle_prunes(
    prunes: usize,
) -> (Result<Rows, (Code, String)>, usize, [Rows; 2]) {
    let scenario = board();
    scenario
        .write(
            ".board/board.json",
            Item::File(r#"{"$type": "board/config/v1", "plugins": ["tasks"]}"#),
        )
        .write("tasks/a.json", Item::File(&task("")));
    let signals = tempfile::tempdir().unwrap();
    let installed = tasks_plugin();
    installed.write(
        "tasks/run",
        Item::Executable(&format!(
            r#"#!/bin/sh
blobs=$(sed -n 's/^ *"blob": "\([0-9a-f]*\)",*$/"\1"/p' | paste -s -d, -)
n=1
while [ -e '{signals}/started-'$n ]; do n=$((n + 1)); done
: > '{signals}/started-'$n
while [ ! -e '{signals}/resume-'$n ]; do sleep 0.01; done
printf '{{"$type": "board/extract-result/v1", "blobs": [%s]}}' "$blobs"
"#,
            signals = signals.path().display(),
        )),
    );
    let plugins = Plugins {
        directory: Some(installed.workdir()),
        environment: board_plugin::Environment {
            path: Some("/usr/bin:/bin".into()),
            ..board_plugin::Environment::default()
        },
        limits: board_plugin::Limits::default(),
    };
    let root = scenario.workdir().to_path_buf();
    let sql = "SELECT path, blob, type, status FROM entries ORDER BY path";
    // A cold index without the plugin answers the same rows.
    let cold = || {
        let board = Board::open(&root, None).unwrap();
        board
            .query(sql, 100, None, &Plugins::default())
            .unwrap()
            .rows
    };
    let before = cold();
    let cache = tempfile::tempdir().unwrap();
    let mut runs = 0;
    let warm = std::thread::scope(|scope| {
        let warm = scope.spawn(|| {
            let board = Board::open(&root, None).unwrap();
            board
                .query(sql, 100, Some(cache.path()), &plugins)
                .map(|report| report.rows)
                .map_err(|error| (error.code(), error.to_string()))
        });
        while !warm.is_finished() {
            let started = signals.path().join(format!("started-{}", runs + 1));
            if !started.exists() {
                std::thread::sleep(std::time::Duration::from_millis(1));
                continue;
            }
            runs += 1;
            let resume = signals.path().join(format!("resume-{runs}"));
            let resumed = Resume(&resume);
            if runs <= prunes.max(1) {
                let title = format!(r#"{{"$type": "lr/task/v1", "title": "u{runs}"}}"#);
                scenario.write("tasks/a.json", Item::File(&title));
                let board = open(&scenario);
                let mut second =
                    Index::open(Some(cache.path()), board.repository().path()).unwrap();
                board.check_in(&mut second, None).unwrap();
                if runs <= prunes {
                    let writer = second.write().unwrap();
                    writer.prune().unwrap();
                    writer.commit().unwrap();
                }
            }
            drop(resumed);
        }
        warm.join().unwrap()
    });
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
fn answered(
    generated: &Generated,
    sql: &str,
    cache: Option<&Path>,
    plugins: &Plugins<'_>,
) -> QueryReport {
    QueryReport {
        memory: None,
        ..open(generated.scenario())
            .query(sql, 10_000, cache, plugins)
            .unwrap()
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    /// Every table answers the same warm and cold through the index, which
    /// check's property test shows; this one covers what `query` adds:
    /// extracting for a plugin and attaching its database.
    #[test]
    fn a_query_answers_from_a_warm_index_as_from_a_cold_one(
        before in generated(),
        after in generated(),
        committed: bool,
    ) {
        let config = r#"{"$type": "board/config/v1", "plugins": ["tasks"]}"#;
        let mut generated = Generated::new(config, &before, committed);
        let installed = tasks_plugin();
        let plugins = Plugins {
            directory: Some(installed.workdir()),
            environment: board_plugin::Environment {
                path: Some("/usr/bin:/bin".into()),
                ..board_plugin::Environment::default()
            },
            limits: board_plugin::Limits::default(),
        };
        let cache = tempfile::tempdir().unwrap();
        answered(&generated, "SELECT 1", Some(cache.path()), &plugins);
        generated.write(&after);
        for sql in [
            "SELECT * FROM entries ORDER BY source, path",
            "SELECT * FROM records ORDER BY source, path",
            "SELECT * FROM extracted ORDER BY plugin, version, blob",
            "SELECT * FROM p_tasks.sqlite_schema",
        ] {
            let warm = answered(&generated, sql, Some(cache.path()), &plugins);
            prop_assert!(warm.plugins.is_empty(), "{:?}", warm.plugins);
            prop_assert_eq!(warm, answered(&generated, sql, None, &plugins), "{}", sql);
        }
    }
}
