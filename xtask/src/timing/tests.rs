use serde_json::json;

use super::*;
use crate::tests::Fixture;

fn filter(text: &str) -> Option<Filter> {
    Filter::parse(text.split_whitespace().map(str::to_owned))
}

fn line(command: &str, version: &str, total: u64, phases: &[(&str, u64)]) -> String {
    let phases: Vec<Value> = (phases.iter())
        .map(|(name, us)| json!({"name": name, "at_us": 0, "us": us, "entered": 1}))
        .collect();
    let (command, capability) = match command.split_once(' ') {
        Some(("call", capability)) => ("call", Some(capability)),
        _ => (command, None),
    };
    let record = json!({"$type": "board/timing/v1", "command": command, "capability": capability,
        "depth": 0, "version": version, "total_us": total, "phases": phases});
    format!("{record}\n")
}

#[test]
fn a_filter_reads_command_words_a_capability_and_a_positive_last() {
    assert_eq!(
        filter("plugin find --last 5"),
        Some(Filter {
            command: "plugin find".to_owned(),
            capability: None,
            last: 5
        })
    );
    let call = filter("call tasks.available").unwrap();
    assert_eq!(
        (call.command.as_str(), call.capability.as_deref()),
        ("call", Some("tasks.available"))
    );
    assert_eq!(filter("tasks.available"), Some(call));
    assert_eq!(filter("query").unwrap().last, 20);
    assert_eq!(filter("query --last 0"), None);
    assert_eq!(filter("--last 3"), None);
}

#[test]
fn the_last_runs_of_the_newest_version_give_the_medians_and_other_runs_are_left_out() {
    let mut log = line("query", "0.0.0+old", 1, &[("snapshot", 1)]);
    for (total, snapshot) in [(50, 40), (10, 4), (30, 20), (20, 10)] {
        log += &line(
            "query",
            "0.0.0+new",
            total,
            &[("open", 1), ("snapshot", snapshot)],
        );
    }
    log += &line("check", "0.0.0+new", 999, &[]);
    log += "not json\n";
    let nested = line("query", "0.0.0+new", 999, &[]).replace(r#""depth":0"#, r#""depth":1"#);
    log += &nested;
    let summary = summarize(&log, &filter("query --last 3").unwrap());
    assert_eq!(
        summary,
        Summary {
            runs: 3,
            version: "0.0.0+new".to_owned(),
            malformed: 1,
            total: 20,
            phases: vec![("open".to_owned(), 1, 3), ("snapshot".to_owned(), 10, 3)],
        }
    );
}

#[test]
fn a_phase_only_some_runs_entered_says_how_many_and_a_capability_chooses_its_calls() {
    let mut log = line("call tasks.available", "v", 10, &[("plugin.call", 8)]);
    log += &line(
        "call tasks.available",
        "v",
        30,
        &[("plugin.call", 9), ("snapshot", 5)],
    );
    log += &line("call plans.open", "v", 99, &[("plugin.call", 99)]);
    let summary = summarize(&log, &filter("call tasks.available").unwrap());
    assert_eq!(summary.runs, 2);
    assert_eq!(
        summary.phases,
        [
            ("plugin.call".to_owned(), 9, 2),
            ("snapshot".to_owned(), 5, 1)
        ]
    );
    let summary = summarize(&log, &filter("call").unwrap());
    assert_eq!(summary.runs, 3);
}

#[test]
fn the_older_log_is_read_before_the_current_one_and_its_last_line_gives_the_version() {
    let fixture = Fixture::new();
    assert_eq!(read(fixture.path()), "");
    assert_eq!(version(fixture.path()), "unknown");
    fixture
        .write("board/timings.jsonl.1", &line("get", "1", 1, &[]))
        .write("board/timings.jsonl", &line("get", "2", 1, &[]));
    assert!(read(fixture.path()).starts_with(&line("get", "1", 1, &[])));
    assert_eq!(version(fixture.path()), "2");
}

#[test]
fn the_report_gives_the_runs_the_version_and_each_median_in_milliseconds() {
    let summary = Summary {
        runs: 3,
        version: "0.0.0+abc".to_owned(),
        malformed: 1,
        total: 20_500,
        phases: vec![
            ("open".to_owned(), 1_000, 3),
            ("snapshot".to_owned(), 10_250, 2),
        ],
    };
    assert_eq!(
        crate::report::render_timing("query", &summary),
        "query: 3 runs of 0.0.0+abc; 1 line of the log is not JSON\n\
         total          20.5 ms\n\
         open            1.0 ms  3 runs\n\
         snapshot       10.2 ms  2 runs\n"
    );
}
