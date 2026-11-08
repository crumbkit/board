//! The params plugin through the binary, with runs enabled beside it: what
//! `params.get` resolves and what `params.due` lists.

use super::*;

/// A board that enables runs and params, holding the heuristic `half-life`
/// with default 14, bounds 3 to 28 and a check that fails above 0.2.
fn board() -> Fixture {
    let board = Fixture::enabling(&["runs", "params"]);
    board.write("params/heuristics/half-life.json", &heuristic(30));
    board
}

/// A heuristic of the half-life in active days, due again after
/// `recheck_after` active days.
fn heuristic(recheck_after: u64) -> String {
    json!({
        "$type": "params/heuristic/v1",
        "summary": "How fast a record's score halves.",
        "unit": "active-days",
        "inputs": [{"name": "touches", "meaning": "Touches per active day.", "source": "Run `board query`."}],
        "method": "Halve the gap between touches.",
        "start": 7,
        "default": 14,
        "bounds": {"min": 3, "max": 28},
        "enough": "Ten touches.",
        "check": {"measure": "The share of archived records touched again.", "fails_above": 0.2},
        "recheck_after": recheck_after,
        "consumer": "controls",
    })
    .to_string()
}

/// A value of `name` derived at `derived`, with `fields` added or replacing
/// its own.
fn value(name: &str, derived: &str, fields: &Value) -> String {
    record(
        json!({
            "$type": "params/value/v1",
            "heuristic": format!("/params/heuristics/{name}"),
            "value": 7,
            "data": "enough",
            "evidence": format!("/suites/params-{name}/runs/a"),
            "derived": derived,
            "by": {"program": "claude-code"},
        }),
        fields,
    )
}

/// A check run of `name`'s suite that started at `started` and measured
/// `check`.
fn check(name: &str, started: &str, check: f64) -> String {
    json!({
        "$type": "runs/run/v1",
        "suite": format!("/suites/params-{name}"),
        "started": started,
        "scope": "full",
        "results": [{"case": "check", "measures": {"check": check}}],
        "measures": {"check": check},
    })
    .to_string()
}

#[test]
fn get_gives_the_default_with_missing_when_no_value_record_exists() {
    let board = board();
    assert_eq!(
        board.call("params.get", &["--name", "half-life"]),
        json!({"name": "half-life", "value": 14.0, "source": "default", "reason": "missing"})
    );
}

#[test]
fn get_gives_the_default_with_too_little_when_the_last_run_found_too_little_data() {
    let board = board();
    let at = "2026-11-08T00:00:00Z";
    board.write(
        "params/values/half-life.json",
        &value("half-life", at, &json!({"data": "too-little"})),
    );
    assert_eq!(
        board.call("params.get", &["--name", "half-life"]),
        json!({"name": "half-life", "value": 14.0, "source": "default", "reason": "too-little"})
    );
}

#[test]
fn get_gives_a_listed_scope_its_own_value_and_an_unlisted_scope_the_records_value() {
    let board = board();
    let at = "2026-11-08T00:00:00Z";
    board.write(
        "params/values/half-life.json",
        &value("half-life", at, &json!({"scopes": {"/repos/board": 21}})),
    );
    let answer = |scope: &[&str]| {
        let flags: Vec<&str> = ["--name", "half-life"]
            .iter()
            .chain(scope)
            .copied()
            .collect();
        board.call("params.get", &flags)
    };
    let resolved = |number: f64| {
        json!({
            "name": "half-life",
            "value": number,
            "source": "value",
            "record": "/params/values/half-life",
            "derived": at,
        })
    };
    assert_eq!(answer(&[]), resolved(7.0));
    assert_eq!(answer(&["--scope", "/repos/board"]), resolved(21.0));
    assert_eq!(answer(&["--scope", "/repos/notes"]), resolved(7.0));
}

#[test]
fn get_refuses_a_name_with_no_heuristic_with_not_found_and_answers_its_twin_with_one() {
    let board = board();
    let refused = board.refusal("params.get", &["--name", "archive-after"]);
    assert_eq!(refused["code"], "params::heuristic::not_found", "{refused}");
    assert_eq!(
        refused["message"],
        "no heuristic for the value archive-after is at /params/heuristics/archive-after"
    );
    board.write("params/heuristics/archive-after.json", &heuristic(30));
    assert_eq!(
        board.call("params.get", &["--name", "archive-after"])["source"],
        "default"
    );
}

#[test]
fn due_lists_a_failed_check_and_a_value_on_the_default_oldest_first_and_leaves_out_a_fresh_passing_one()
 {
    let board = board();
    for name in ["failing", "passing", "too-little", "missing"] {
        board.write(&format!("params/heuristics/{name}.json"), &heuristic(30));
    }
    board
        .write(
            "params/values/failing.json",
            &value("failing", "2026-11-02T00:00:00Z", &json!({})),
        )
        .write(
            "params/values/passing.json",
            &value("passing", "2026-11-01T00:00:00Z", &json!({})),
        )
        .write(
            "params/values/too-little.json",
            &value(
                "too-little",
                "2026-11-03T00:00:00Z",
                &json!({"data": "too-little"}),
            ),
        )
        .write(
            "params/values/half-life.json",
            &value("half-life", "2026-11-04T00:00:00Z", &json!({})),
        )
        // The failing value's latest check failed; an earlier one passed.
        .write(
            "suites/params-failing/runs/a.json",
            &check("failing", "2026-11-05T00:00:00Z", 0.1),
        )
        .write(
            "suites/params-failing/runs/b.json",
            &check("failing", "2026-11-06T00:00:00Z", 0.3),
        )
        // The passing value's latest check passed, after one that failed.
        .write(
            "suites/params-passing/runs/a.json",
            &check("passing", "2026-11-05T00:00:00Z", 0.3),
        )
        .write(
            "suites/params-passing/runs/b.json",
            &check("passing", "2026-11-06T00:00:00Z", 0.2),
        )
        // The half-life check failed before the value was derived again.
        .write(
            "suites/params-half-life/runs/a.json",
            &check("half-life", "2026-11-03T00:00:00Z", 0.9),
        );
    assert_eq!(
        board.call("params.due", &[]),
        json!({
            "due": [
                {"name": "missing", "reason": "on-default"},
                {"name": "failing", "reason": "check-failed"},
                {"name": "too-little", "reason": "on-default"},
            ],
            "checked": 5,
        })
    );
}

#[test]
fn due_lists_nothing_when_every_value_is_fresh_and_its_check_passes() {
    let board = board();
    board.write(
        "params/values/half-life.json",
        &value("half-life", "2026-11-04T00:00:00Z", &json!({})),
    );
    assert_eq!(
        board.call("params.due", &[]),
        json!({"due": [], "checked": 1})
    );
}
