use std::time::Duration;

use serde_json::json;

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
        took: Took::Skipped("the board does not enable the tasks plugin".to_owned()),
    };
    assert!(skipped.passed());
    assert_eq!(
        crate::report::render_bench(&[skipped]),
        "skip  the board does not enable the tasks plugin  `board call tasks.available --repo /`, warm\n"
    );
}

#[test]
fn bench_reads_a_board_or_a_record_count() {
    let parse = |text: &str| Options::parse(text.split_whitespace().map(str::to_owned));
    assert_eq!(
        parse("--records 50"),
        Some(Options {
            board: None,
            records: 50,
            against: None,
            runs: None,
        })
    );
    assert_eq!(parse("--board /b").unwrap().board, Some("/b".into()));
    let against = parse("--against /old/board --runs 3").unwrap();
    assert_eq!(
        (against.against, against.runs),
        (Some("/old/board".into()), Some(3))
    );
    assert_eq!(parse("--runs 0"), None);
    assert_eq!(parse("--records many"), None);
    assert_eq!(parse("--other"), None);
}

#[test]
fn against_names_an_existing_base_and_takes_runs_only_with_it_or_is_refused() {
    let fixture = Fixture::new();
    fixture.write("board", "");
    let base = fixture.path().join("board");
    let options = |against: Option<PathBuf>, runs| Options {
        board: None,
        records: 1,
        against,
        runs,
    };
    let missing = options(Some(fixture.path().join("nothing")), None);
    assert_eq!(
        missing.against().unwrap_err().code(),
        "xtask::bench::no_against"
    );
    let named = options(Some(base.clone()), None);
    assert_eq!(named.against().unwrap(), Some((base.as_path(), 15)));
    let lone = options(None, Some(3)).against().unwrap_err();
    assert_eq!(lone.code(), "xtask::bench::runs_without_against");
    let paired = options(Some(base.clone()), Some(3));
    assert_eq!(paired.against().unwrap(), Some((base.as_path(), 3)));
    assert_eq!(options(None, None).against().unwrap(), None);
}

#[test]
fn a_generated_board_holds_valid_open_tasks_and_one_in_five_waits_on_the_first() {
    let root = tempfile::tempdir().unwrap();
    generate(root.path(), 0, &[], 0).unwrap();
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
fn a_generated_mounted_board_mounts_each_repository_at_its_pinned_commit_and_every_entry_is_ok() {
    let root = tempfile::tempdir().unwrap();
    let board = generate_mounted(root.path(), 1_000).unwrap();
    let pinned: Vec<String> = ["r00", "r01"]
        .map(|name| {
            let main = root
                .path()
                .join(format!("repos/{name}/.git/refs/heads/main"));
            std::fs::read_to_string(main).unwrap().trim().to_owned()
        })
        .to_vec();
    let cache = tempfile::tempdir().unwrap();
    let query = |sql: &str| {
        let board = Board::open(&board, None).unwrap();
        board
            .query(sql, 100, None, Some(cache.path()))
            .unwrap()
            .rows
    };
    let sources = query(
        "SELECT source, count(*) FROM entries WHERE source != 'worktree' GROUP BY source ORDER BY source",
    );
    // Each repository: 25 tasks, 100 projects, 100 of the plugin's tasks,
    // 100 source files, its config and three schemas.
    let mut expected: Vec<_> = (pinned.iter())
        .map(|commit| vec![json!(commit), json!(329)])
        .collect();
    expected.sort_by_key(|row| row[0].to_string());
    assert_eq!(sources, expected);
    assert_eq!(
        query("SELECT count(*), count(code) FROM entries"),
        [[json!(1_064), json!(0)]]
    );
    let across = "SELECT count(*) FROM records WHERE type = 'bench/task/v1' \
        AND json_extract(body, '$.project') LIKE '/repos/r01/projects/%' AND code IS NULL";
    assert_eq!(query(across), [[json!(100)]]);
}

#[test]
fn the_median_is_the_middle_time_once_sorted() {
    let times = [5, 1, 4, 2, 3].map(Duration::from_millis).to_vec();
    assert_eq!(median(times), Duration::from_millis(3));
}

#[test]
fn the_plugin_call_is_timed_on_a_board_that_enables_tasks_and_skipped_on_one_that_does_not() {
    let root = tempfile::tempdir().unwrap();
    generate(root.path(), 1, &[], 0).unwrap();
    let budgets = [(CALL.to_owned(), Duration::from_secs(1))];
    let timed = run(root.path(), &budgets, Some(root.path())).unwrap();
    assert!(matches!(timed[0].took, Took::Median(_)), "{timed:?}");
    let config = r#"{"$type": "board/config/v1"}"#;
    let plain = tempfile::tempdir().unwrap();
    let extra = [(".board/board.json".to_owned(), config.to_owned())];
    generate(plain.path(), 1, &extra, 0).unwrap();
    let skipped = run(plain.path(), &budgets, Some(plain.path())).unwrap();
    assert_eq!(
        skipped[0].took,
        Took::Skipped("the board does not enable the tasks plugin".to_owned())
    );
}

#[test]
fn a_generated_board_is_committed_and_a_put_commits_onto_it_as_its_own_identity() {
    let root = tempfile::tempdir().unwrap();
    generate(root.path(), 1, &[], 0).unwrap();
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
    generate(root.path(), 1, &[], 0).unwrap();
    let budgets = [(PUT.to_owned(), Duration::from_secs(1))];
    let given = run(root.path(), &budgets, None).unwrap();
    assert_eq!(
        given[0].took,
        Took::Skipped(
            "the board was given with --board, and bench writes only a board it generated"
                .to_owned()
        )
    );
    let timed = run(root.path(), &budgets, Some(root.path())).unwrap();
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

#[test]
fn the_mounted_query_row_is_timed_on_the_generated_mounted_board_and_skipped_on_a_given_one() {
    let root = tempfile::tempdir().unwrap();
    let mounted = generate_mounted(root.path(), 20).unwrap();
    let budgets = [(MOUNTED.to_owned(), Duration::from_secs(1))];
    let given = run(&mounted, &budgets, None).unwrap();
    assert_eq!(
        given[0].took,
        Took::Skipped(
            "the board was given with --board, and bench mounts repositories only into a board it generated"
                .to_owned()
        )
    );
    let timed = run(&mounted, &budgets, Some(&mounted)).unwrap();
    assert!(matches!(timed[0].took, Took::Median(_)), "{timed:?}");
}

#[test]
fn the_sample_is_the_first_valid_record_and_its_directory_skipping_files_and_invalid_records() {
    let root = tempfile::tempdir().unwrap();
    // `.a/` sorts before every generated entry, the first of which is the
    // board's config record.
    let extra = [
        (".a/invalid.json", r#"{"$type": "bench/task/v1"}"#),
        (".a/notes.txt", "notes"),
    ]
    .map(|(path, text)| (path.to_owned(), text.to_owned()));
    generate(root.path(), 0, &extra, 0).unwrap();
    let (sample, directory) = sample(&warmed(root.path()).unwrap().2).unwrap();
    assert_eq!(
        (sample.as_str(), directory.as_str()),
        ("/.board/board", "/.board")
    );
}

#[test]
fn the_query_row_indexes_the_board_into_the_cache_it_is_given() {
    let root = tempfile::tempdir().unwrap();
    generate(root.path(), 0, &[], 0).unwrap();
    let cache = tempfile::tempdir().unwrap();
    count(&Board::open(root.path(), None).unwrap(), cache.path()).unwrap();
    let written = std::fs::read_dir(cache.path()).unwrap().count();
    assert_ne!(written, 0);
}

#[test]
fn settling_waits_over_a_second() {
    let started = Instant::now();
    settle();
    assert!(started.elapsed() > Duration::from_secs(1));
}
