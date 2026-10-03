use std::time::Duration;

use super::*;

fn budget(millis: u64, provisional: bool) -> Budget {
    Budget {
        duration: Duration::from_millis(millis),
        provisional,
    }
}

const DOCS: &str = "## Performance

| Operation | Budget |
| --- | --- |
| `board get`, warm | 50 ms |
| full index, cold | 10 s, provisional |

The timing job reports every run.
";

#[test]
fn the_budgets_are_read_from_the_docs_table() {
    assert_eq!(
        budgets(DOCS).unwrap(),
        [
            ("`board get`, warm".to_owned(), budget(50, false)),
            ("full index, cold".to_owned(), budget(10_000, true)),
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
    let timed = |median, provisional| Timed {
        operation: "x".to_owned(),
        budget: budget(100, provisional),
        median: Duration::from_millis(median),
    };
    assert!(timed(300, false).passed());
    assert!(!timed(301, false).passed());
    assert!(
        timed(301, true).passed(),
        "a provisional budget is only reported"
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
fn every_budgeted_operation_can_be_timed_on_a_small_generated_board() {
    let root = tempfile::tempdir().unwrap();
    generate(root.path(), 20).unwrap();
    let docs = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/architecture.md"
    ))
    .unwrap();
    let timed = run(root.path(), &budgets(&docs).unwrap()).unwrap();
    assert_eq!(timed.len(), 4);
}
