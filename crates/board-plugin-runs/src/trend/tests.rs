//! `trend` answers with one measure of the qualifying runs, as the Python
//! plugin did.

use serde_json::json;

use super::*;
use crate::qualifying::tests::{Real, board, called, run, suite};

/// The run that started on day `n` of October 2026, with `fields`.
fn day(n: u8, fields: &Value) -> String {
    run(&format!("2026-10-0{n}T09:00:00Z"), fields)
}

/// `trend`'s values for `input` about `/suites/a`, and its other counts.
fn values(host: &Real, input: &Value) -> (Vec<Value>, Value) {
    let mut asked = json!({"suite": "/suites/a"});
    for (name, value) in input.as_object().unwrap() {
        asked[name] = value.clone();
    }
    let mut output = trend(host, &asked).unwrap();
    let points = output["points"].as_array().unwrap().iter();
    let values = points.map(|point| point["value"].clone()).collect();
    output.as_object_mut().unwrap().remove("points");
    (values, output)
}

#[test]
fn trend_gives_a_runs_number_newest_first_and_counts_runs_whose_measure_is_a_flag_label_or_missing()
{
    let (_scenario, host) = board(&[
        ("suites/a.json", suite()),
        (
            "suites/a/runs/1.json",
            day(1, &json!({"measures": {"wall_s": 50}})),
        ),
        (
            "suites/a/runs/2.json",
            day(2, &json!({"measures": {"wall_s": 42.5}})),
        ),
        (
            "suites/a/runs/3.json",
            day(3, &json!({"measures": {"tests": 9}})),
        ),
        (
            "suites/a/runs/4.json",
            day(4, &json!({"measures": {"wall_s": "fast"}})),
        ),
        (
            "suites/a/runs/5.json",
            day(5, &json!({"measures": {"wall_s": true}})),
        ),
        (
            "suites/a/runs/6.json",
            day(6, &json!({"measures": {"wall_s": 1}, "dirty": true})),
        ),
    ]);
    let output = trend(&host, &json!({"suite": "/suites/a", "measure": "wall_s"})).unwrap();
    assert_eq!(
        output,
        json!({
            "points": [
                {"path": "/suites/a/runs/2", "started": "2026-10-02T09:00:00Z", "value": 42.5},
                {"path": "/suites/a/runs/1", "started": "2026-10-01T09:00:00Z", "value": 50.0},
            ],
            "total": 2,
            "truncated": false,
            "without_measure": 3,
            "not_counted": 1,
            "unreadable": 0,
        })
    );
}

#[test]
fn a_case_gives_its_first_lowest_trials_measure_and_a_case_no_run_lists_gives_none() {
    let results = json!([
        {"case": "cold", "trial": 2, "measures": {"ms": 50}},
        {"case": "cold", "trial": 1, "measures": {"ms": 40}},
        {"case": "cold", "trial": 1, "measures": {"ms": 45}},
        {"case": "warm", "measures": {"ms": 9}},
    ]);
    let fields = json!({"results": results, "measures": {"ms": 99}});
    let (_scenario, host) = board(&[
        ("suites/a.json", suite()),
        ("suites/a/runs/1.json", day(1, &fields)),
    ]);
    let without = |n: usize| json!(n);
    let cases = [
        (
            json!({"measure": "ms", "case": "cold"}),
            vec![json!(40.0)],
            0,
        ),
        (
            json!({"measure": "ms", "case": "warm"}),
            vec![json!(9.0)],
            0,
        ),
        (json!({"measure": "ms"}), vec![json!(99.0)], 0),
        (json!({"measure": "ms", "case": "list"}), vec![], 1),
    ];
    for (input, expected, missing) in cases {
        let (values, output) = values(&host, &input);
        assert_eq!(values, expected, "{input}");
        assert_eq!(output["without_measure"], without(missing), "{input}");
    }
}

#[test]
fn board_calls_runs_trend_in_its_own_process_showing_at_most_its_limit() {
    let files = [
        ("suites/a.json", suite()),
        (
            "suites/a/runs/1.json",
            day(1, &json!({"measures": {"s": 1}})),
        ),
        (
            "suites/a/runs/2.json",
            day(2, &json!({"measures": {"s": 2}})),
        ),
        (
            "suites/a/runs/3.json",
            day(3, &json!({"measures": {"s": 3}})),
        ),
    ];
    for (limit, kept, truncated) in [(Some(2), 2, true), (Some(3), 3, false), (None, 3, false)] {
        let mut input = json!({"suite": "/suites/a", "measure": "s"});
        if let Some(limit) = limit {
            input["limit"] = json!(limit);
        }
        let (output, started) = called(&files, "trend", &input);
        assert!(!started);
        assert_eq!(output["points"].as_array().unwrap().len(), kept, "{input}");
        assert_eq!(
            (&output["total"], &output["truncated"]),
            (&json!(3), &json!(truncated))
        );
        assert_eq!(output["points"][0]["value"], json!(3.0));
    }
}
