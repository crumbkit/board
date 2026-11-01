//! The runs plugin through the binary: what `runs.latest` and `runs.trend`
//! answer, and what the shipped schemas admit. These were the plugin's
//! pytest parity suite.

use super::*;

const INVALID: Option<&str> = Some("board::record::invalid");

fn board() -> Fixture {
    Fixture::enabling(&["runs"])
}

fn suite(fields: &Value) -> String {
    record(
        json!({"$type": "runs/suite/v1", "subjects": ["board"], "method": "Run it."}),
        fields,
    )
}

/// A full run of a clean revision of `suite` that started at `started` and
/// passed, with `fields` added or replacing its own.
fn run(suite: &str, started: &str, fields: &Value) -> String {
    record(
        json!({
            "$type": "runs/run/v1",
            "suite": suite,
            "by": "claude",
            "started": started,
            "scope": "full",
            "dirty": false,
            "results": [],
        }),
        fields,
    )
}

/// A run of `/suites/a` on day `day` of October 2026, with `fields`.
fn day(day: u8, fields: &Value) -> String {
    run("/suites/a", &format!("2026-10-0{day}T09:00:00Z"), fields)
}

fn latest(board: &Fixture, suite: &str) -> Value {
    board.call("runs.latest", &["--suite", suite])
}

/// `latest`'s output without `run.age_s`, which grows with the clock.
fn ageless(mut output: Value) -> Value {
    if let Some(run) = output.get_mut("run").and_then(Value::as_object_mut) {
        run.remove("age_s");
    }
    output
}

fn failed() -> Value {
    json!({"case": "slow", "outcome": "fail", "note": "slow took 31 s, over its 30 s limit"})
}

fn erred() -> Value {
    json!({"case": "boot", "outcome": "error", "note": "boot could not bind its port"})
}

fn skipped() -> Value {
    json!({"case": "net", "outcome": "skipped", "note": "net needs a network"})
}

#[test]
fn latest_returns_the_newest_full_clean_run_and_counts_the_targeted_and_dirty_ones() {
    let board = board();
    let second = json!({
        "finished": "2026-10-02T09:05:00Z",
        "results": [failed(), skipped(), erred()],
        "measures": {"tests": 40, "failures": 1},
    });
    board
        .write("suites/a.json", &suite(&json!({})))
        .write("suites/a/runs/1.json", &day(1, &json!({})))
        .write("suites/a/runs/2.json", &day(2, &second))
        .write(
            "suites/a/runs/3.json",
            &day(3, &json!({"scope": "targeted"})),
        )
        .write("suites/a/runs/4.json", &day(4, &json!({"dirty": true})));
    let output = latest(&board, "/suites/a");
    let age = output["run"]["age_s"].as_u64().unwrap();
    assert_eq!(
        ageless(output.clone()),
        json!({
            "run": {
                "path": "/suites/a/runs/2",
                "started": "2026-10-02T09:00:00Z",
                "finished": "2026-10-02T09:05:00Z",
                "outcome": "fail",
                "measures": {"tests": 40, "failures": 1},
                "failed": ["slow", "boot"],
            },
            "not_counted": 2,
            "unreadable": 0,
        })
    );
    assert!(output["run"]["measures"]["tests"].is_u64());
    // 2026-10-02T09:05:00Z, in seconds since the epoch.
    let finished = 1_790_931_900;
    let since = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        - finished;
    assert!(since - 60 < age && age <= since, "{age} {since}");
}

#[test]
fn a_run_whose_only_unpassed_cases_were_skipped_passes() {
    let board = board();
    board.write("suites/a.json", &suite(&json!({}))).write(
        "suites/a/runs/1.json",
        &day(1, &json!({"results": [skipped()]})),
    );
    assert_eq!(latest(&board, "/suites/a")["run"]["outcome"], "pass");
    board.write(
        "suites/a/runs/1.json",
        &day(1, &json!({"results": [skipped(), erred()]})),
    );
    assert_eq!(latest(&board, "/suites/a")["run"]["outcome"], "fail");
}

#[test]
fn a_case_that_failed_in_several_trials_is_listed_once() {
    let board = board();
    let results = json!([
        {"case": "cold", "trial": 1, "outcome": "fail", "note": "timed out"},
        {"case": "cold", "trial": 2, "outcome": "fail", "note": "timed out"},
        {"case": "warm", "trial": 1, "outcome": "error", "note": "crashed"},
    ]);
    board.write("suites/a.json", &suite(&json!({}))).write(
        "suites/a/runs/1.json",
        &day(1, &json!({"results": results})),
    );
    assert_eq!(
        latest(&board, "/suites/a")["run"]["failed"],
        json!(["cold", "warm"])
    );
}

#[test]
fn latest_says_no_run_qualifies_when_every_run_is_targeted_dirty_or_silent_on_dirt() {
    let board = board();
    let mut silent: Value = serde_json::from_str(&day(9, &json!({}))).unwrap();
    silent.as_object_mut().unwrap().remove("dirty");
    board
        .write("suites/a.json", &suite(&json!({})))
        .write(
            "suites/a/runs/1.json",
            &day(1, &json!({"scope": "targeted"})),
        )
        .write("suites/a/runs/2.json", &day(1, &json!({"dirty": true})))
        .write("suites/a/runs/4.json", &silent.to_string());
    assert_eq!(
        latest(&board, "/suites/a"),
        json!({"run": null, "not_counted": 3, "unreadable": 0})
    );
    board.write("suites/a/runs/3.json", &day(1, &json!({})));
    assert_eq!(
        latest(&board, "/suites/a")["run"]["path"],
        "/suites/a/runs/3"
    );
}

#[test]
fn a_run_board_marks_degraded_is_counted_as_unreadable_and_a_valid_one_is_the_latest() {
    let board = board();
    board
        .write("suites/a.json", &suite(&json!({})))
        .write("suites/a/runs/1.json", &day(1, &json!({})))
        .write(
            "suites/a/runs/2.json",
            &day(2, &json!({"scope": "partial"})),
        );
    let output = latest(&board, "/suites/a");
    assert_eq!(
        (&output["run"]["path"], &output["unreadable"]),
        (&json!("/suites/a/runs/1"), &json!(1))
    );
    board.write("suites/a/runs/2.json", &day(2, &json!({})));
    let output = latest(&board, "/suites/a");
    assert_eq!(
        (&output["run"]["path"], &output["unreadable"]),
        (&json!("/suites/a/runs/2"), &json!(0))
    );
}

#[test]
fn runs_that_started_at_once_are_ordered_by_path_last_first() {
    let board = board();
    board
        .write("suites/a.json", &suite(&json!({})))
        .write("suites/a/runs/20261001T090000Z-a.json", &day(1, &json!({})))
        .write("suites/a/runs/20261001T090000Z-b.json", &day(1, &json!({})));
    assert_eq!(
        latest(&board, "/suites/a")["run"]["path"],
        "/suites/a/runs/20261001T090000Z-b"
    );
}

#[test]
fn latest_reads_only_the_suites_own_runs() {
    let board = board();
    board
        .write("suites/a.json", &suite(&json!({})))
        .write("suites/ab.json", &suite(&json!({})))
        .write("suites/a/runs/1.json", &day(1, &json!({})))
        .write(
            "suites/ab/runs/1.json",
            &run("/suites/ab", "2026-10-09T09:00:00Z", &json!({})),
        );
    assert_eq!(
        latest(&board, "/suites/a")["run"]["path"],
        "/suites/a/runs/1"
    );
}

#[test]
fn a_suite_that_does_not_exist_is_refused_and_one_that_does_is_answered() {
    let board = board();
    board.write("suites/a/runs/1.json", &day(1, &json!({})));
    for (target, flags) in [
        ("runs.latest", &["--suite", "/suites/a"][..]),
        ("runs.trend", &["--suite", "/suites/a", "--measure", "s"]),
    ] {
        assert_eq!(
            board.refusal(target, flags)["code"],
            "runs::suite::not_found"
        );
    }
    board.write("suites/a.json", &suite(&json!({})));
    assert_eq!(
        latest(&board, "/suites/a")["run"]["path"],
        "/suites/a/runs/1"
    );
    let trend = board.call("runs.trend", &["--suite", "/suites/a", "--measure", "s"]);
    assert_eq!(trend["total"], 0);
}

#[test]
fn latest_reads_a_suite_in_a_mounted_board() {
    let board = board();
    let (ci, commit) = repository(
        &["runs"],
        &[
            ("suites/ci.json", suite(&json!({}))),
            (
                "suites/ci/runs/1.json",
                run("/suites/ci", "2026-10-01T09:00:00Z", &json!({})),
            ),
            (
                "suites/ci/runs/2.json",
                run("/suites/ci", "2026-10-02T09:00:00Z", &json!({})),
            ),
        ],
    );
    mount(&board, "boards/ci.json", &ci, commit);
    assert_eq!(
        latest(&board, "/boards/ci/suites/ci")["run"]["path"],
        "/boards/ci/suites/ci/runs/2"
    );
}

#[test]
fn latest_and_trend_answer_the_same_from_a_cold_cache() {
    let board = board();
    board
        .write("suites/a.json", &suite(&json!({})))
        .write(
            "suites/a/runs/1.json",
            &day(1, &json!({"results": [failed()], "measures": {"s": 1}})),
        )
        .write(
            "suites/a/runs/2.json",
            &day(2, &json!({"results": [failed()], "measures": {"s": 2}})),
        )
        .write("suites/a/runs/3.json", &day(3, &json!({"dirty": true})));
    for (target, flags) in [
        ("runs.latest", &["--suite", "/suites/a"][..]),
        ("runs.trend", &["--suite", "/suites/a", "--measure", "s"]),
    ] {
        let warm = ageless(board.call(target, flags));
        assert_eq!(ageless(board.answer(target, flags, true)), warm, "{target}");
    }
}

/// The values of the points `runs.trend` gives with `flags` for `/suites/a`,
/// and its output.
fn trend(board: &Fixture, suite: &str, flags: &[&str]) -> (Vec<Value>, Value) {
    let flags: Vec<&str> = ["--suite", suite]
        .into_iter()
        .chain(flags.iter().copied())
        .collect();
    let output = board.call("runs.trend", &flags);
    let values = (output["points"].as_array().unwrap().iter())
        .map(|point| point["value"].clone())
        .collect();
    (values, output)
}

#[test]
fn trend_returns_a_runs_measure_newest_first_from_full_clean_runs_only() {
    let board = board();
    board
        .write("suites/a.json", &suite(&json!({})))
        .write(
            "suites/a/runs/1.json",
            &day(1, &json!({"measures": {"wall_s": 50}})),
        )
        .write(
            "suites/a/runs/2.json",
            &day(2, &json!({"measures": {"wall_s": 42.5}})),
        )
        .write(
            "suites/a/runs/3.json",
            &day(3, &json!({"measures": {"tests": 9}})),
        )
        .write(
            "suites/a/runs/4.json",
            &day(4, &json!({"measures": {"wall_s": "fast"}})),
        )
        .write(
            "suites/a/runs/5.json",
            &day(5, &json!({"measures": {"wall_s": true}})),
        )
        .write(
            "suites/a/runs/6.json",
            &day(6, &json!({"measures": {"wall_s": 1}, "dirty": true})),
        );
    let (_, output) = trend(&board, "/suites/a", &["--measure", "wall_s"]);
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
fn trend_reads_one_cases_measure_when_given_a_case() {
    let board = board();
    board.write(
        "suites/bench.json",
        &suite(&json!({"measures": [{"name": "median_ms", "unit": "ms"}]})),
    );
    for (day, get) in [(1, 4.0), (2, 3.5)] {
        let fields = json!({
            "results": [
                {"case": "get", "measures": {"median_ms": get}},
                {"case": "put", "measures": {"median_ms": 20}},
            ],
            "measures": {"median_ms": 99},
        });
        let started = format!("2026-10-0{day}T09:00:00Z");
        board.write(
            &format!("suites/bench/runs/{day}.json"),
            &run("/suites/bench", &started, &fields),
        );
    }
    let measure = ["--measure", "median_ms"];
    let with = |case: &[&str]| {
        let flags: Vec<&str> = measure.iter().chain(case).copied().collect();
        trend(&board, "/suites/bench", &flags)
    };
    assert_eq!(with(&["--case", "get"]).0, [json!(3.5), json!(4.0)]);
    assert_eq!(with(&[]).0, [json!(99.0), json!(99.0)]);
    let (values, output) = with(&["--case", "list"]);
    assert!(values.is_empty());
    assert_eq!(output["without_measure"], 2);
}

#[test]
fn trend_reads_the_first_trial_of_a_case_that_ran_several() {
    let board = board();
    let results = json!([
        {"case": "cold", "trial": 2, "measures": {"wall_ms": 50}},
        {"case": "cold", "trial": 1, "measures": {"wall_ms": 40}},
    ]);
    board.write("suites/a.json", &suite(&json!({}))).write(
        "suites/a/runs/1.json",
        &day(1, &json!({"results": results})),
    );
    let (values, _) = trend(
        &board,
        "/suites/a",
        &["--measure", "wall_ms", "--case", "cold"],
    );
    assert_eq!(values, [json!(40.0)]);
}

#[test]
fn trend_shows_at_most_its_limit_and_says_how_many_it_left_out() {
    let board = board();
    board.write("suites/a.json", &suite(&json!({})));
    for n in 1..=3 {
        board.write(
            &format!("suites/a/runs/{n}.json"),
            &day(n, &json!({"measures": {"s": n}})),
        );
    }
    let (values, output) = trend(&board, "/suites/a", &["--measure", "s", "--limit", "2"]);
    assert_eq!(values, [json!(3.0), json!(2.0)]);
    assert_eq!(
        (&output["total"], &output["truncated"]),
        (&json!(3), &json!(true))
    );
}

#[test]
fn board_lists_runs_as_enabled_and_enabling_wrote_the_schemas_it_ships() {
    let board = board();
    let report = board.json(&["plugin", "list"]);
    let plugin = (report["data"].as_array().unwrap().iter())
        .find(|plugin| plugin["name"] == "runs")
        .unwrap();
    assert_eq!(
        (&plugin["status"], &plugin["enabled"]),
        (&json!("ok"), &json!(true))
    );
    assert_eq!(plugin.get("run"), None);
    let shipped = shipped("runs");
    assert_eq!(shipped.len(), 2, "suite and run");
    for (path, text) in shipped {
        assert_eq!(board.read(&path), text, "{path}");
    }
}

#[test]
fn call_help_prints_each_flag_of_trend_with_its_sentence() {
    let board = board();
    let help = board.run(&["call", "runs.trend", "-h"]);
    assert_eq!(help.code, 0, "{}", text(&help.stderr));
    let help = text(&help.stdout);
    for flag in [
        "--suite <suite>",
        "--measure <measure>",
        "--case <case>",
        "--limit <limit>",
    ] {
        assert!(help.contains(flag), "{flag}: {help}");
    }
    assert!(
        help.contains("The measure's name, such as median_ms."),
        "{help}"
    );
}

#[test]
fn a_suite_lists_its_cases_inline_or_in_a_file_and_is_refused_with_both() {
    let board = board();
    board
        .write("suites/cases.txt", "a\n")
        .write(
            "suites/inline.json",
            &suite(&json!({"cases": [{"id": "a"}]})),
        )
        .write(
            "suites/file.json",
            &suite(&json!({"cases_file": "/suites/cases.txt"})),
        )
        .write(
            "suites/found.json",
            &suite(&json!({"command": "cargo test"})),
        )
        .write(
            "suites/both.json",
            &suite(&json!({"cases": [{"id": "a"}], "cases_file": "/suites/cases.txt"})),
        );
    assert_eq!(
        board.checked("/suites/"),
        codes(&[
            ("/suites/cases.txt", None),
            ("/suites/inline", None),
            ("/suites/file", None),
            ("/suites/found", None),
            ("/suites/both", INVALID),
        ])
    );
}

#[test]
fn a_suites_measure_needs_a_unit() {
    let board = board();
    board
        .write(
            "suites/with.json",
            &suite(&json!({"measures": [{"name": "median_ms", "unit": "ms"}]})),
        )
        .write(
            "suites/without.json",
            &suite(&json!({"measures": [{"name": "median_ms"}]})),
        );
    assert_eq!(
        board.checked("/suites/"),
        codes(&[("/suites/with", None), ("/suites/without", INVALID)])
    );
}

#[test]
fn a_case_that_did_not_pass_needs_a_note_and_a_measured_case_does_not() {
    let board = board();
    let results = |result: Value| json!({"results": [result]});
    board
        .write("suites/a.json", &suite(&json!({})))
        .write(
            "suites/a/runs/noted.json",
            &day(
                1,
                &results(json!({"case": "a", "outcome": "fail", "note": "a timed out after 30 s"})),
            ),
        )
        .write(
            "suites/a/runs/silent.json",
            &day(1, &results(json!({"case": "a", "outcome": "fail"}))),
        )
        .write(
            "suites/a/runs/skipped.json",
            &day(1, &results(json!({"case": "a", "outcome": "skipped"}))),
        )
        .write(
            "suites/a/runs/measured.json",
            &day(1, &results(json!({"case": "a", "measures": {"ms": 4}}))),
        )
        .write(
            "suites/a/runs/empty.json",
            &day(1, &results(json!({"case": "a"}))),
        );
    assert_eq!(
        board.checked("/suites/"),
        codes(&[
            ("/suites/a", None),
            ("/suites/a/runs/noted", None),
            ("/suites/a/runs/silent", INVALID),
            ("/suites/a/runs/skipped", INVALID),
            ("/suites/a/runs/measured", None),
            ("/suites/a/runs/empty", INVALID),
        ])
    );
}

#[test]
fn a_run_is_full_or_targeted_names_its_host_whole_and_gives_its_times_zone() {
    let board = board();
    let host = json!({"os": "macos", "arch": "aarch64", "logical_cpus": 12});
    board
        .write("suites/a.json", &suite(&json!({})))
        .write("suites/a/runs/full.json", &day(1, &json!({"host": host})))
        .write(
            "suites/a/runs/targeted.json",
            &day(1, &json!({"scope": "targeted"})),
        )
        .write(
            "suites/a/runs/partial.json",
            &day(1, &json!({"scope": "partial"})),
        )
        .write(
            "suites/a/runs/no-cpus.json",
            &day(1, &json!({"host": {"os": "macos", "arch": "aarch64"}})),
        )
        .write(
            "suites/a/runs/no-zone.json",
            &run("/suites/a", "2026-10-01T09:00:00", &json!({})),
        );
    assert_eq!(
        board.checked("/suites/"),
        codes(&[
            ("/suites/a", None),
            ("/suites/a/runs/full", None),
            ("/suites/a/runs/targeted", None),
            ("/suites/a/runs/partial", INVALID),
            ("/suites/a/runs/no-cpus", INVALID),
            ("/suites/a/runs/no-zone", INVALID),
        ])
    );
}

#[test]
fn a_result_numbers_its_trial_from_one_and_its_evidence_must_exist() {
    let board = board();
    let first = json!({"case": "a", "outcome": "pass", "trial": 1, "evidence": ["/suites/a/evidence/log.txt"]});
    let with = |fields: Value| {
        let result: Value = serde_json::from_str(&record(first.clone(), &fields)).unwrap();
        json!([result])
    };
    board
        .write("suites/a.json", &suite(&json!({})))
        .write("suites/a/evidence/log.txt", "ok\n")
        .write(
            "suites/a/runs/first.json",
            &day(
                1,
                &json!({"results": [first], "setup": "M4 Max", "summary": "All passed."}),
            ),
        )
        .write(
            "suites/a/runs/zeroth.json",
            &day(1, &json!({"results": with(json!({"trial": 0}))})),
        )
        .write(
            "suites/a/runs/lost.json",
            &day(
                1,
                &json!({"results": with(json!({"evidence": ["/suites/a/gone.txt"]}))}),
            ),
        );
    assert_eq!(
        board.checked("/suites/"),
        codes(&[
            ("/suites/a", None),
            ("/suites/a/evidence/log.txt", None),
            ("/suites/a/runs/first", None),
            ("/suites/a/runs/zeroth", INVALID),
            ("/suites/a/runs/lost", Some("board::ref::dangling")),
        ])
    );
}

#[test]
fn a_runs_suite_must_be_a_suite() {
    let board = board();
    board
        .write("suites/a.json", &suite(&json!({})))
        .write("suites/notes.md", "# Notes\n")
        .write("suites/a/runs/right.json", &day(1, &json!({})))
        .write(
            "suites/a/runs/wrong.json",
            &run("/suites/notes.md", "2026-10-01T09:00:00Z", &json!({})),
        )
        .write(
            "suites/a/runs/gone.json",
            &run("/suites/gone", "2026-10-01T09:00:00Z", &json!({})),
        );
    assert_eq!(
        board.checked("/suites/"),
        codes(&[
            ("/suites/a", None),
            ("/suites/notes.md", None),
            ("/suites/a/runs/right", None),
            ("/suites/a/runs/wrong", Some("board::ref::wrong_type")),
            ("/suites/a/runs/gone", Some("board::ref::dangling")),
        ])
    );
}

#[test]
fn a_run_a_hook_wrote_names_the_plugin_and_hook_and_nothing_else() {
    let board = board();
    board
        .write("suites/a.json", &suite(&json!({})))
        .write(
            "suites/a/runs/hooked.json",
            &day(1, &json!({"written_by": "runs/subagent-stop"})),
        )
        .write(
            "suites/a/runs/prose.json",
            &day(1, &json!({"written_by": "the subagent hook"})),
        )
        .write(
            "suites/a/runs/empty.json",
            &day(1, &json!({"written_by": ""})),
        );
    assert_eq!(
        board.checked("/suites/"),
        codes(&[
            ("/suites/a", None),
            ("/suites/a/runs/hooked", None),
            ("/suites/a/runs/prose", INVALID),
            ("/suites/a/runs/empty", INVALID),
        ])
    );
}
