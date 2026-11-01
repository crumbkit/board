//! `latest` answers with the newest qualifying run, as the Python plugin did.

use std::time::Duration;

use serde_json::json;

use super::*;
use crate::qualifying::tests::{board, run, suite};

/// `seconds` after 1970-01-01T00:00:00Z.
fn at(seconds: u64) -> std::time::SystemTime {
    std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(seconds)
}

#[test]
fn latest_gives_the_newest_runs_outcome_failed_cases_once_measures_and_age_from_its_finish() {
    let results = json!([
        {"case": "cold", "trial": 1, "outcome": "fail", "note": "timed out"},
        {"case": "net", "outcome": "skipped", "note": "no network"},
        {"case": "cold", "trial": 2, "outcome": "fail", "note": "timed out"},
        {"case": "boot", "outcome": "error", "note": "crashed"},
    ]);
    let (_scenario, mut host) = board(&[
        ("suites/a.json", suite()),
        (
            "suites/a/runs/1.json",
            run("1970-01-01T00:00:00Z", &json!({})),
        ),
        (
            "suites/a/runs/2.json",
            run(
                "1970-01-01T00:01:00+00:00",
                &json!({"finished": "1970-01-01T01:01:40.25+01:00", "results": results, "measures": {"tests": 40, "wall_s": 2.0}}),
            ),
        ),
        (
            "suites/a/runs/3.json",
            run("1970-01-02T00:00:00Z", &json!({"scope": "targeted"})),
        ),
    ]);
    host.settings.now = at(150);
    let output = latest(&host, &json!({"suite": "/suites/a"})).unwrap();
    assert_eq!(
        output,
        json!({
            "run": {
                "path": "/suites/a/runs/2",
                "started": "1970-01-01T00:01:00Z",
                "finished": "1970-01-01T01:01:40.250000+01:00",
                "outcome": "fail",
                "measures": {"tests": 40, "wall_s": 2.0},
                "failed": ["cold", "boot"],
                "age_s": 49,
            },
            "not_counted": 1,
            "unreadable": 0,
        })
    );
    assert!(output["run"]["measures"]["tests"].is_u64());
    assert!(output["run"]["measures"]["wall_s"].is_f64());
}

#[test]
fn a_run_whose_only_unpassed_cases_were_skipped_passes_and_its_age_runs_from_its_start() {
    let skipped = json!([{"case": "net", "outcome": "skipped", "note": "no network"}]);
    let (_scenario, mut host) = board(&[
        ("suites/a.json", suite()),
        (
            "suites/a/runs/1.json",
            run("1970-01-01T00:00:10Z", &json!({"results": skipped})),
        ),
    ]);
    host.settings.now = at(15);
    let output = latest(&host, &json!({"suite": "/suites/a"})).unwrap();
    assert_eq!(
        output["run"],
        json!({
            "path": "/suites/a/runs/1",
            "started": "1970-01-01T00:00:10Z",
            "finished": null,
            "outcome": "pass",
            "measures": {},
            "failed": [],
            "age_s": 5,
        })
    );
}

#[test]
fn latest_says_no_run_qualifies_with_null_and_counts_what_it_left_out() {
    let (_scenario, host) = board(&[
        ("suites/a.json", suite()),
        (
            "suites/a/runs/1.json",
            run("1970-01-01T00:00:00Z", &json!({"dirty": true})),
        ),
        (
            "suites/a/runs/2.json",
            run("1970-01-01T00:00:00Z", &json!({"scope": "partial"})),
        ),
    ]);
    let output = latest(&host, &json!({"suite": "/suites/a"})).unwrap();
    assert_eq!(
        output,
        json!({"run": null, "not_counted": 1, "unreadable": 1})
    );
}
