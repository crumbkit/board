use std::time::Instant;

use board_model::{Code, Fields, Refusal};

use clap::CommandFactory as _;

use super::*;
use crate::render::{self, Format};

fn ran(command: &str) -> Ran {
    Ran {
        command: Some(command.to_owned()),
        root: Some(PathBuf::from("/boards/one")),
        ..Ran::default()
    }
}

#[test]
fn the_command_is_the_subcommands_named_without_any_value() {
    let matches = crate::run::Cli::command()
        .try_get_matches_from([
            "board",
            "--json",
            "plugin",
            "find",
            "--capability",
            "secret",
        ])
        .unwrap();
    assert_eq!(command(&matches), "plugin find");
}

#[test]
fn a_record_is_written_as_these_exact_bytes_but_for_its_times() {
    let mut ran = ran("check");
    ran.trace.add("snapshot", Instant::now());
    ran.trace.add("snapshot", Instant::now());
    ran.trace.count("entries", 3);
    ran.trace.count("parsed", 0);
    let output = Output {
        code: 1,
        error: Some("board::path::not_found".to_owned()),
        ..Output::default()
    };
    let mut record = record(Instant::now(), &ran, &output, 1).unwrap();
    for (pointer, time) in [
        ("/at_ms", 1_791_250_685_132_u64),
        ("/total_us", 1280),
        ("/phases/0/at_us", 149),
        ("/phases/0/us", 1125),
    ] {
        *record.pointer_mut(pointer).unwrap() = time.into();
    }
    let golden = concat!(
        r#"{"$type":"board/timing/v1","at_ms":1791250685132,"command":"check","depth":1,"#,
        r#""error":"board::path::not_found","exit":1,"#,
        r#""phases":[{"at_us":149,"entered":2,"name":"snapshot","us":1125}],"#,
        r#""root":"/boards/one","total_us":1280,"version":"0.0.0","#,
        r#""work":{"entries":3,"parsed":0}}"#,
    );
    assert_eq!(record.to_string(), golden);
}

#[test]
fn a_run_whose_command_line_was_not_parsed_has_no_record_and_a_parsed_one_has_one() {
    let output = Output::default();
    assert_eq!(record(Instant::now(), &Ran::default(), &output, 0), None);
    assert!(record(Instant::now(), &ran("get"), &output, 0).is_some());
}

#[test]
fn phases_are_recorded_in_the_order_they_started_whatever_order_they_ended() {
    let started = Instant::now();
    let mut ran = ran("check");
    let outer = started + Duration::from_micros(10);
    let inner = started + Duration::from_micros(20);
    // An inner phase ends, and so is traced, before the phase around it.
    ran.trace.add("index.refresh", inner);
    ran.trace.add("snapshot", outer);
    let record = record(started, &ran, &Output::default(), 0).unwrap();
    let phases = &record["phases"];
    assert_eq!(
        (&phases[0]["name"], &phases[1]["name"]),
        (&json!("snapshot"), &json!("index.refresh"))
    );
    assert_eq!(
        (&phases[0]["at_us"], &phases[1]["at_us"]),
        (&json!(10), &json!(20))
    );
}

#[test]
fn of_two_phases_that_started_in_the_same_tick_the_longer_one_around_the_other_comes_first() {
    let started = Instant::now();
    let mut ran = ran("check");
    for (name, took) in [("index.refresh", 5), ("snapshot", 9)] {
        ran.trace.phases.push(board_ops::Phase {
            name,
            started,
            took: Duration::from_micros(took),
            entered: 1,
        });
    }
    let record = record(started, &ran, &Output::default(), 0).unwrap();
    let phases = &record["phases"];
    assert_eq!(
        (&phases[0]["name"], &phases[1]["name"]),
        (&json!("snapshot"), &json!("index.refresh"))
    );
}

#[test]
fn a_duration_is_recorded_in_whole_microseconds() {
    assert_eq!(micros(Duration::from_nanos(2_500_999)), 2_500);
    assert_eq!(micros(Duration::ZERO), 0);
}

#[test]
fn a_refusal_names_its_code_for_the_record_in_text_and_json() {
    let boards = Refusal::new(Code::PutInvalid, "refused", &Fields::default());
    let plugins = Refusal::plugin(
        "tasks::date::bad".to_owned(),
        "bad date".to_owned(),
        serde_json::Map::new(),
    );
    for format in [Format::Text, Format::Json] {
        let refused = render::refused(format, &boards);
        assert_eq!(refused.error.as_deref(), Some("board::put::invalid"));
        let refused = render::refused(format, &plugins);
        assert_eq!(refused.error.as_deref(), Some("tasks::date::bad"));
    }
}
