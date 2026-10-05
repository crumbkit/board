//! Querying the index with SQL.

use std::path::Path;

use board_model::Code;
use board_scenario::{Item, Scenario};
use proptest::prelude::*;
use serde_json::json;

use crate::tests::generated::{Generated, generated};
use crate::tests::{board, open, task};
use crate::{Plugins, QueryReport};

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
            "SELECT * FROM extracted ORDER BY plugin, version, blob",
            "SELECT * FROM p_tasks.sqlite_schema",
        ] {
            let warm = answered(&generated, sql, Some(cache.path()), &plugins);
            prop_assert!(warm.plugins.is_empty(), "{:?}", warm.plugins);
            prop_assert_eq!(warm, answered(&generated, sql, None, &plugins), "{}", sql);
        }
    }
}
