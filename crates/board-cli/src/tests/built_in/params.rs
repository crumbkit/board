//! The params plugin through the binary, with runs enabled beside it: what
//! `params.get` resolves and what `params.due` lists.

use super::*;

/// A `recheck_after` no value in these tests reaches, about 2,700 years.
const NEVER: u64 = 1_000_000;

/// A board that enables runs and params, holding the heuristic `half-life`
/// with default 14, bounds 3 to 28 and a check that fails above 0.2.
fn board() -> Fixture {
    let board = Fixture::enabling(&["runs", "params"]);
    board.write("params/heuristics/half-life.json", &heuristic(NEVER));
    board
}

/// A heuristic of the half-life, due again after `recheck_after` calendar
/// days.
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
            "heuristic": name,
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

/// What `params.due` lists, without the two heuristics every board has.
fn due(board: &Fixture) -> Value {
    let mut report = board.call("params.due", &[]);
    (report["due"].as_array_mut().unwrap())
        .retain(|entry| !entry["name"].as_str().unwrap().starts_with("overflow-"));
    report["due"].clone()
}

/// `params.get` for `half-life`.
fn half_life(board: &Fixture) -> Value {
    board.call("params.get", &["--name", "half-life"])
}

/// The half-life value record, derived a fixed time ago, with `fields`
/// added or replacing its own.
fn write_half_life(board: &Fixture, fields: &Value) {
    board.write(
        "params/values/half-life.json",
        &value("half-life", "2026-11-08T00:00:00Z", fields),
    );
}

#[test]
fn get_gives_the_default_with_missing_when_no_value_record_exists() {
    let board = board();
    assert_eq!(
        half_life(&board),
        json!({
            "name": "half-life",
            "value": 14.0,
            "source": "default",
            "reason": "missing",
            "warning": {
                "code": "params::value::missing",
                "message": "no value record is at /params/values/half-life, so the default 14 applies",
            },
        })
    );
}

#[test]
fn get_gives_the_default_with_too_little_when_the_last_run_found_too_little_data() {
    let board = board();
    write_half_life(&board, &json!({"data": "too-little"}));
    assert_eq!(
        half_life(&board),
        json!({"name": "half-life", "value": 14.0, "source": "default", "reason": "too-little"})
    );
}

#[test]
fn a_value_below_its_bounds_gives_the_default_and_comes_due_and_one_at_the_bound_is_used_and_not_due()
 {
    let board = board();
    write_half_life(&board, &json!({"value": 2}));
    assert_eq!(
        half_life(&board),
        json!({
            "name": "half-life",
            "value": 14.0,
            "source": "default",
            "reason": "out-of-bounds",
            "warning": {
                "code": "params::value::out_of_bounds",
                "message": "the value half-life is 2, outside its heuristic's bounds 3 to 28, so the default 14 applies",
            },
        })
    );
    assert_eq!(
        due(&board),
        json!([{"name": "half-life", "reason": "out-of-bounds"}])
    );

    write_half_life(&board, &json!({"value": 3}));
    assert_eq!(
        half_life(&board),
        json!({
            "name": "half-life",
            "value": 3.0,
            "source": "value",
            "record": "/params/values/half-life",
            "derived": "2026-11-08T00:00:00Z",
        })
    );
    assert_eq!(due(&board), json!([]));
}

#[test]
fn a_value_naming_another_heuristic_gives_the_default_and_comes_due_and_its_twin_is_used_and_not_due()
 {
    let board = board();
    board.write("params/heuristics/archive-after.json", &heuristic(NEVER));
    write_half_life(&board, &json!({"heuristic": "archive-after"}));
    assert_eq!(
        half_life(&board),
        json!({
            "name": "half-life",
            "value": 14.0,
            "source": "default",
            "reason": "wrong-heuristic",
            "warning": {
                "code": "params::value::wrong_heuristic",
                "message": "the value half-life names the heuristic archive-after, not half-life, so the default 14 applies",
            },
        })
    );
    let listed = due(&board);
    assert!(
        (listed.as_array().unwrap().iter())
            .any(|entry| entry == &json!({"name": "half-life", "reason": "wrong-heuristic"})),
        "{listed}"
    );

    write_half_life(&board, &json!({}));
    assert_eq!(half_life(&board)["source"], "value");
    assert_eq!(
        due(&board),
        json!([{"name": "archive-after", "reason": "on-default"}])
    );
}

#[test]
fn put_writes_a_value_outside_its_bounds_and_readers_fall_back() {
    let board = board();
    board.write(
        "suites/params-half-life/runs/a.json",
        &check("half-life", "2026-11-08T00:00:00Z", 0.1),
    );
    let record = value("half-life", "2026-11-08T00:00:00Z", &json!({"value": 2}));
    let put = board.run_with(&["put", "/params/values/half-life"], &record);
    assert_eq!(put.code, 0, "{}", text(&put.stderr));
    assert_eq!(half_life(&board)["reason"], "out-of-bounds");
}

#[test]
fn a_board_with_no_heuristic_records_resolves_both_overflow_values_from_the_shipped_defaults() {
    let board = Fixture::enabling(&["runs", "params"]);
    for (name, default) in [
        ("overflow-agent-grace", 200.0),
        ("overflow-owner-grace", 800.0),
    ] {
        let got = board.call("params.get", &["--name", name]);
        assert_eq!(
            (&got["value"], &got["source"], &got["reason"]),
            (&json!(default), &json!("default"), &json!("missing")),
            "{got}"
        );
    }
    assert_eq!(
        board.call("params.due", &[]),
        json!({
            "due": [
                {"name": "overflow-agent-grace", "reason": "on-default"},
                {"name": "overflow-owner-grace", "reason": "on-default"},
            ],
            "checked": 2,
        })
    );
}

#[test]
fn a_record_overrides_the_shipped_heuristic_of_its_name_and_leaves_the_other_shipped() {
    let board = Fixture::enabling(&["runs", "params"]);
    let mut own: Value = serde_json::from_str(&heuristic(NEVER)).unwrap();
    own["default"] = json!(300);
    own["bounds"] = json!({"min": 1, "max": 400});
    board.write(
        "params/heuristics/overflow-agent-grace.json",
        &own.to_string(),
    );
    let answer = |name| board.call("params.get", &["--name", name])["value"].clone();
    assert_eq!(answer("overflow-agent-grace"), json!(300.0));
    assert_eq!(answer("overflow-owner-grace"), json!(800.0));
    assert_eq!(board.call("params.due", &[])["checked"], 2);
}

#[test]
fn check_reports_nothing_degraded_for_a_value_of_a_shipped_heuristic_with_no_heuristic_record() {
    let board = Fixture::enabling(&["runs", "params"]);
    let name = "overflow-agent-grace";
    board
        .write(
            &format!("suites/params-{name}.json"),
            &json!({
                "$type": "runs/suite/v1",
                "subjects": ["crumbkit/board"],
                "method": "Run the heuristic's check.",
            })
            .to_string(),
        )
        .write(
            &format!("suites/params-{name}/runs/a.json"),
            &check(name, "2026-11-08T00:00:00Z", 0.1),
        )
        .write(
            &format!("params/values/{name}.json"),
            &value(name, "2026-11-08T00:00:00Z", &json!({"value": 150})),
        );
    let checked = board.checked("");
    assert!(
        checked.contains_key(&format!("/params/values/{name}")),
        "{checked:?}"
    );
    assert!(checked.values().all(Option::is_none), "{checked:?}");
}

#[test]
fn a_value_of_a_shipped_heuristic_is_written_by_put_and_read_with_no_heuristic_record() {
    let board = Fixture::enabling(&["runs", "params"]);
    board.write(
        "suites/params-overflow-agent-grace/runs/a.json",
        &check("overflow-agent-grace", "2026-11-08T00:00:00Z", 0.1),
    );
    let record = value(
        "overflow-agent-grace",
        "2026-11-08T00:00:00Z",
        &json!({"value": 150}),
    );
    let put = board.run_with(&["put", "/params/values/overflow-agent-grace"], &record);
    assert_eq!(put.code, 0, "{}", text(&put.stderr));
    let got = board.call("params.get", &["--name", "overflow-agent-grace"]);
    assert_eq!(
        (&got["value"], &got["source"]),
        (&json!(150.0), &json!("value")),
        "{got}"
    );
}

#[test]
fn get_gives_a_listed_scope_its_own_value_and_an_unlisted_scope_the_records_value() {
    let board = board();
    let at = "2026-11-08T00:00:00Z";
    write_half_life(&board, &json!({"scopes": {"/repos/board": 21}}));
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
        "no heuristic for the value archive-after is shipped or at /params/heuristics/archive-after"
    );
    board.write("params/heuristics/archive-after.json", &heuristic(NEVER));
    assert_eq!(
        board.call("params.get", &["--name", "archive-after"])["source"],
        "default"
    );
}

#[test]
fn due_lists_a_failed_check_a_stale_value_and_one_on_the_default_oldest_first_and_leaves_out_a_fresh_passing_one()
 {
    let board = board();
    for name in ["failing", "passing", "too-little", "missing"] {
        board.write(&format!("params/heuristics/{name}.json"), &heuristic(NEVER));
    }
    board
        .write("params/heuristics/aged.json", &heuristic(1))
        .write(
            "params/values/aged.json",
            &value("aged", "2026-10-01T00:00:00Z", &json!({})),
        )
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
        due(&board),
        json!([
            {"name": "missing", "reason": "on-default"},
            {"name": "aged", "reason": "stale"},
            {"name": "failing", "reason": "check-failed"},
            {"name": "too-little", "reason": "on-default"},
        ])
    );
    // The six heuristics written here and the two the plugin ships.
    assert_eq!(board.call("params.due", &[])["checked"], 8);
}

#[test]
fn due_lists_nothing_of_a_fresh_value_whose_check_passes_or_that_a_recheck_does_not_reach() {
    let board = board();
    write_half_life(&board, &json!({}));
    assert_eq!(due(&board), json!([]));
    board
        .write("params/heuristics/half-life.json", &heuristic(1))
        .write(
            "params/values/half-life.json",
            &value("half-life", "2026-01-01T00:00:00Z", &json!({})),
        );
    assert_eq!(
        due(&board),
        json!([{"name": "half-life", "reason": "stale"}])
    );
}
