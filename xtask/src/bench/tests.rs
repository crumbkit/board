use std::time::Duration;

use super::*;
use crate::tests::Fixture;

const DOCS: &str = "## Performance

| Operation | Budget |
| --- | --- |
| `board get`, warm | 50 ms |
| full index, cold | 10 s |

The timing job reports every run.
";

#[test]
fn the_budgets_are_read_from_the_docs_table() {
    assert_eq!(
        budgets(DOCS).unwrap(),
        [
            ("`board get`, warm".to_owned(), Duration::from_millis(50)),
            ("full index, cold".to_owned(), Duration::from_secs(10)),
        ]
    );
}

#[test]
fn a_budget_row_without_a_unit_is_refused_and_one_with_is_admitted() {
    let bad = DOCS.replace("| 50 ms |", "| 50 |");
    assert!(matches!(budgets(&bad), Err(BenchError::Budget { .. })));
    assert!(budgets(DOCS).is_ok());
    assert!(matches!(budgets("no table"), Err(BenchError::NoBudgets)));
}

#[test]
fn an_operation_passes_up_to_three_times_its_budget() {
    let timed = |median| Timed {
        operation: "x".to_owned(),
        budget: Duration::from_millis(100),
        took: Took::Median(Duration::from_millis(median)),
    };
    assert!(timed(300).passed());
    assert!(!timed(301).passed());
}

#[test]
fn a_skipped_operation_passes_and_its_report_line_says_why() {
    let skipped = Timed {
        operation: CALL.to_owned(),
        budget: Duration::from_millis(100),
        took: Took::Skipped("uv is not on PATH".to_owned()),
    };
    assert!(skipped.passed());
    assert_eq!(
        crate::report::render_bench(&[skipped]),
        "skip  uv is not on PATH  `board call tasks.available --repo /`, warm\n"
    );
}

#[test]
fn bench_reads_a_board_or_a_record_count() {
    let parse = |text: &str| Options::parse(text.split_whitespace().map(str::to_owned));
    assert_eq!(
        parse("--records 50"),
        Some(Options {
            board: None,
            records: 50
        })
    );
    assert_eq!(parse("--board /b").unwrap().board, Some("/b".into()));
    assert_eq!(parse("--records many"), None);
    assert_eq!(parse("--other"), None);
}

#[test]
fn a_generated_board_holds_valid_open_tasks_and_one_in_five_waits_on_the_first() {
    let root = tempfile::tempdir().unwrap();
    generate(root.path(), 0).unwrap();
    let cache = tempfile::tempdir().unwrap();
    let sql = "SELECT count(*), count(json_extract(body, '$.after')) FROM records \
        WHERE type = 'tasks/task/v2' AND code IS NULL \
        AND json_extract(body, '$.status') = 'open'";
    let report = Board::open(root.path(), None)
        .unwrap()
        .query(sql, 1, None, Some(cache.path()))
        .unwrap();
    assert_eq!(report.rows, [[100, 20]]);
}

#[test]
fn the_median_is_the_middle_time_once_sorted() {
    let times = [5, 1, 4, 2, 3].map(Duration::from_millis).to_vec();
    assert_eq!(median(times), Duration::from_millis(3));
}

/// A host whose `PATH` holds a stand-in `uv` then the system's tools, whose
/// board directory holds a stand-in `board`, and whose tasks plugin is `sh`
/// running `script`.
fn host(fixture: &Fixture, script: &str) -> Host {
    let manifest = r#"{"$type": "board/plugin-manifest/v1", "name": "tasks", "version": "0.1.0",
        "description": "", "run": ["sh", "./run"], "capabilities": [{"name": "available",
        "description": "", "inputSchema": {"type": "object"}, "outputSchema": {"type": "object"}}]}"#;
    fixture
        .write("bin/uv", "")
        .write("target/board", "")
        .write("data/board/plugins/tasks/plugin.json", manifest)
        .write("data/board/plugins/tasks/run", script);
    let directories = [fixture.path().join("bin"), "/usr/bin".into(), "/bin".into()];
    Host {
        path: std::env::join_paths(directories).unwrap(),
        home: Some("/home/nobody".into()),
        board: fixture.path().join("target"),
        data: fixture.path().join("data"),
    }
}

#[test]
fn the_plugin_call_is_skipped_without_tasks_or_uv_and_runs_with_both() {
    let fixture = Fixture::new();
    let ready = host(&fixture, "");
    assert_eq!(skip(true, &ready), None);
    assert_eq!(
        skip(false, &ready),
        Some("the board does not enable the tasks plugin".to_owned())
    );
    let no_uv = Host {
        path: "/usr/bin:/bin".into(),
        ..host(&fixture, "")
    };
    assert_eq!(skip(true, &no_uv), Some("uv is not on PATH".to_owned()));
}

#[test]
fn a_missing_board_is_refused_and_one_beside_xtask_goes_first_on_path() {
    let fixture = Fixture::new();
    let directory = fixture.path().display();
    let path = plugin_path(&host(&fixture, "")).unwrap();
    assert_eq!(
        path,
        OsString::from(format!("{directory}/target:{directory}/bin:/usr/bin:/bin"))
    );
    let no_board = Host {
        board: fixture.path().join("bin"),
        ..host(&fixture, "")
    };
    let refused = plugin_path(&no_board).unwrap_err();
    assert_eq!(refused.code(), "xtask::bench::no_board");
    assert_eq!(
        refused.to_string(),
        format!(
            "{directory}/bin holds no `board`; build it with xtask's profile first, \
             as in `cargo build --release --package board-cli`"
        )
    );
}

#[test]
fn a_plugin_call_that_fails_fails_the_bench_and_one_that_answers_is_timed() {
    let root = tempfile::tempdir().unwrap();
    generate(root.path(), 1).unwrap();
    let call = |fixture: &Fixture, script: &str| {
        let budgets = [(CALL.to_owned(), Duration::from_secs(1))];
        run(
            root.path(),
            &budgets,
            &host(fixture, script),
            Corpus::Generated,
        )
    };
    let failed = call(&Fixture::new(), "#!/bin/sh\ncat > /dev/null\nexit 1\n").unwrap_err();
    assert_eq!(failed.code(), "xtask::bench::failed");
    assert!(
        failed.to_string().contains("tasks failed with exit 1"),
        "{failed}"
    );
    let fixture = Fixture::new();
    let answer = r#"{"$type": "board/call-result/v1", "output": {}}"#;
    let script = format!("#!/bin/sh\ncat > /dev/null\nenv > env.txt\nprintf '%s' '{answer}'\n");
    let timed = call(&fixture, &script).unwrap();
    assert!(matches!(timed[0].took, Took::Median(_)), "{timed:?}");
    // The plugin's own `board` runs need this board, its index and plugins.
    let seen =
        std::fs::read_to_string(fixture.path().join("data/board/plugins/tasks/env.txt")).unwrap();
    let directory = fixture.path().display();
    for line in [
        format!("PATH={directory}/target:{directory}/bin:/usr/bin:/bin\n"),
        "HOME=/home/nobody\n".to_owned(),
        format!("XDG_DATA_HOME={directory}/data\n"),
        "XDG_CACHE_HOME=/".to_owned(),
    ] {
        assert!(seen.contains(&line), "{line} in {seen}");
    }
}

#[test]
fn a_generated_board_is_committed_and_a_put_commits_onto_it_as_its_own_identity() {
    let root = tempfile::tempdir().unwrap();
    generate(root.path(), 1).unwrap();
    let main = root.path().join(".git/refs/heads/main");
    let generated = std::fs::read_to_string(&main).unwrap();
    assert_eq!(generated.trim().len(), 40, "{generated:?}");
    let board = Board::open(root.path(), None).unwrap();
    let report = board
        .put(
            &"/put/first".parse().unwrap(),
            board_ops::Expect::Absent,
            br#"{"$type": "bench/task/v1", "title": "first", "project": "/projects/p0"}"#,
        )
        .unwrap();
    let committed = report.commit.unwrap().to_string();
    assert_ne!(committed, generated.trim());
    assert_eq!(std::fs::read_to_string(&main).unwrap().trim(), committed);
}

#[test]
fn the_put_row_writes_a_record_each_run_on_a_generated_board_and_skips_a_given_one() {
    let root = tempfile::tempdir().unwrap();
    generate(root.path(), 1).unwrap();
    let budgets = [(PUT.to_owned(), Duration::from_secs(1))];
    let host = host(&Fixture::new(), "");
    let given = run(root.path(), &budgets, &host, Corpus::Given).unwrap();
    assert_eq!(
        given[0].took,
        Took::Skipped(
            "the board was given with --board, and bench writes only a board it generated"
                .to_owned()
        )
    );
    let timed = run(root.path(), &budgets, &host, Corpus::Generated).unwrap();
    assert!(matches!(timed[0].took, Took::Median(_)), "{timed:?}");
    let cache = tempfile::tempdir().unwrap();
    let report = Board::open(root.path(), None)
        .unwrap()
        .query(
            "SELECT count(*) FROM records WHERE path LIKE 'put/%' AND code IS NULL",
            1,
            None,
            Some(cache.path()),
        )
        .unwrap();
    assert_eq!(report.rows, [[5]]);
    assert_eq!(RUNS, 5);
}
