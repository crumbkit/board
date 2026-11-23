//! Each rule of the migration gives its kind, the first rule that matches
//! wins, and a suite that changes before the write refuses the whole batch.

use std::rc::Rc;

use board_scenario::Item;

use super::*;
use crate::qualifying::tests::{board, suite};

/// Neighbours with the eval `/evals/named`, an eval listing
/// `/suites/listed`, and a zero rule naming `/suites/zeroed` and
/// `/suites/named`.
fn neighbours() -> Neighbours {
    Neighbours {
        evals: vec!["/evals/named".to_owned(), "/evals/lister".to_owned()],
        listed: vec!["/suites/listed".to_owned()],
        zeroed: vec![
            "/suites/zeroed".to_owned(),
            "/suites/named-review".to_owned(),
        ],
    }
}

#[test]
fn each_rule_gives_its_kind_and_a_name_no_rule_matches_gives_none() {
    let neighbours = neighbours();
    let standard = "Audited by the apply-standard skill against STANDARDS.md.";
    for (path, method, expected) in [
        (
            "/suites/zeroed",
            "Run it.",
            Some((SuiteKind::Compliance, ChosenBy::ZeroRule)),
        ),
        (
            "/suites/app",
            standard,
            Some((SuiteKind::Compliance, ChosenBy::ApplyStandard)),
        ),
        (
            "/suites/named",
            "Run it.",
            Some((SuiteKind::Eval, ChosenBy::EvalName)),
        ),
        (
            "/suites/listed",
            "Run it.",
            Some((SuiteKind::Eval, ChosenBy::EvalSuites)),
        ),
        (
            "/suites/app-review",
            "Run it.",
            Some((SuiteKind::Compliance, ChosenBy::ReviewName)),
        ),
        (
            "/suites/board-gates",
            "Run it.",
            Some((SuiteKind::Test, ChosenBy::TestName)),
        ),
        (
            "/suites/board-ci",
            "Run it.",
            Some((SuiteKind::Test, ChosenBy::TestName)),
        ),
        (
            "/suites/board-checks",
            "Run it.",
            Some((SuiteKind::Test, ChosenBy::TestName)),
        ),
        (
            "/suites/board-pr-lane",
            "Run it.",
            Some((SuiteKind::Test, ChosenBy::TestName)),
        ),
        (
            "/suites/board-ready",
            "Run it.",
            Some((SuiteKind::Test, ChosenBy::TestName)),
        ),
        (
            "/suites/core-tests",
            "Run it.",
            Some((SuiteKind::Test, ChosenBy::TestName)),
        ),
        (
            "/suites/bench-get",
            "Run it.",
            Some((SuiteKind::Benchmark, ChosenBy::BenchmarkName)),
        ),
        (
            "/suites/put-latency",
            "Run it.",
            Some((SuiteKind::Benchmark, ChosenBy::BenchmarkName)),
        ),
        (
            "/suites/put-timing",
            "Run it.",
            Some((SuiteKind::Benchmark, ChosenBy::BenchmarkName)),
        ),
        (
            "/suites/put-benchmark",
            "Run it.",
            Some((SuiteKind::Benchmark, ChosenBy::BenchmarkName)),
        ),
        (
            "/suites/hook-overhead",
            "Run it.",
            Some((SuiteKind::Benchmark, ChosenBy::BenchmarkName)),
        ),
        (
            "/suites/model-cost",
            "Run it.",
            Some((SuiteKind::Benchmark, ChosenBy::BenchmarkName)),
        ),
        ("/suites/claude-code", "Run it.", None),
        ("/suites/benchmark", "Run it.", None),
        ("/suites/gates", "Run it.", None),
        ("/suites/tests", "Run it.", None),
        ("/suites/phase-stop-test-flakes", "Run it.", None),
        ("/suites/app", "Audited by hand.", None),
    ] {
        assert_eq!(neighbours.classify(path, method), expected, "{path}");
    }
}

#[test]
fn the_first_rule_that_matches_wins() {
    let neighbours = Neighbours {
        evals: vec![
            "/evals/registry-review".to_owned(),
            "/evals/x-gates".to_owned(),
        ],
        listed: vec!["/suites/bench-listed".to_owned()],
        zeroed: vec!["/suites/x-gates".to_owned(), "/suites/x-tests".to_owned()],
    };
    for (path, expected) in [
        // An eval's suite named like an audit's is the eval's.
        (
            "/suites/registry-review",
            (SuiteKind::Eval, ChosenBy::EvalName),
        ),
        (
            "/suites/x-gates",
            (SuiteKind::Compliance, ChosenBy::ZeroRule),
        ),
        (
            "/suites/x-tests",
            (SuiteKind::Compliance, ChosenBy::ZeroRule),
        ),
        (
            "/suites/bench-listed",
            (SuiteKind::Eval, ChosenBy::EvalSuites),
        ),
    ] {
        assert_eq!(
            neighbours.classify(path, "Run it."),
            Some(expected),
            "{path}"
        );
    }
}

#[test]
fn a_suite_that_changes_before_the_write_refuses_the_batch_and_nothing_is_written() {
    for changed in [true, false] {
        let (scenario, host) = board(&[
            ("suites/a-gates.json", suite()),
            ("suites/bench-b.json", suite()),
        ]);
        let scenario = Rc::new(scenario);
        if changed {
            let there = Rc::clone(&scenario);
            *host.between.borrow_mut() = Some(Box::new(move || {
                let edited = json!({"$type": "runs/suite/v1", "method": "Run it again."});
                there.write("suites/bench-b.json", Item::File(&edited.to_string()));
            }));
        }
        let result = migrate(&host, &json!({"apply": true}));
        let read = |name: &str| -> Value {
            let text = std::fs::read_to_string(scenario.workdir().join(name)).unwrap();
            serde_json::from_str(&text).unwrap()
        };
        if changed {
            let refused = result.unwrap_err();
            assert_eq!(refused.code, "runs::board::refused");
            assert_eq!(refused.fields["code"], json!("board::put::stale"));
            assert_eq!(read("suites/a-gates.json").get("kind"), None);
        } else {
            let output = result.unwrap();
            assert_eq!(output["counts"]["test"], 1);
            assert_eq!(read("suites/a-gates.json")["kind"], "test");
            assert_eq!(read("suites/bench-b.json")["kind"], "benchmark");
        }
    }
}
