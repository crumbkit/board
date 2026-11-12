//! How a version rule compares the versions it captures with its floor.

use super::*;

fn rule(capture: &str) -> Version {
    Version {
        at_least: "1.99.1".to_owned(),
        capture: capture.to_owned(),
    }
}

fn found(capture: &str, text: &str) -> Vec<(usize, String)> {
    let expression = compile("/controls/rust", &rule(capture)).expect("the capture compiles");
    (behind(&expression, "1.99.1", text).into_iter())
        .map(|behind| (behind.line, behind.found))
        .collect()
}

#[test]
fn a_version_older_than_the_floor_is_behind_and_names_its_line() {
    let text = "[toolchain]\nchannel = \"1.97.1\"\n";
    assert_eq!(
        found(r#"channel = "([0-9.]+)""#, text),
        [(2, "1.97.1".to_owned())]
    );
}

#[test]
fn a_version_at_or_above_the_floor_is_not_behind() {
    let capture = r#"channel = "([0-9.]+)""#;
    assert_eq!(found(capture, "channel = \"1.99.1\""), []);
    assert_eq!(found(capture, "channel = \"1.100.0\""), []);
}

#[test]
fn a_pin_of_fewer_numbers_is_compared_over_only_the_numbers_it_names() {
    let capture = r"node:([0-9.]+)";
    assert_eq!(found(capture, "FROM node:1"), []);
    assert_eq!(found(capture, "FROM node:1.99"), []);
    assert_eq!(found(capture, "FROM node:1.98"), [(1, "1.98".to_owned())]);
}

#[test]
fn a_capture_that_is_not_a_dotted_number_is_skipped() {
    let capture = r#"channel = "([^"]+)""#;
    assert_eq!(found(capture, "channel = \"stable\""), []);
    assert_eq!(
        found(capture, "channel = \"1.97.1\""),
        [(1, "1.97.1".to_owned())]
    );
}

#[test]
fn every_capture_in_a_file_is_compared() {
    let text = "FROM rust:1.97.1 AS build\nFROM rust:1.99.1\nFROM rust:1.98.0\n";
    assert_eq!(
        found(r"rust:([0-9.]+)", text),
        [(1, "1.97.1".to_owned()), (3, "1.98.0".to_owned())]
    );
}

#[test]
fn a_capture_without_a_group_is_refused() {
    let refused = compile("/controls/rust", &rule(r"channel = .*")).unwrap_err();
    assert_eq!(refused.code, "controls::capture::invalid");
    assert!(compile("/controls/rust", &rule(r"channel = (.*)")).is_ok());
}

#[test]
fn a_capture_that_does_not_compile_is_refused() {
    let refused = compile("/controls/rust", &rule(r"channel = (")).unwrap_err();
    assert_eq!(refused.code, "controls::capture::invalid");
    assert!(compile("/controls/rust", &rule(r"channel = ()")).is_ok());
}

#[test]
fn every_version_a_capture_finds_is_counted_older_or_not_and_a_capture_that_is_no_version_is_not() {
    let expression = compile("/controls/rust", &rule(r"rust:([0-9a-z.]+)")).unwrap();
    let text = "rust:1.97\nrust:1.100 rust:latest\n";
    assert_eq!(super::found(&expression, text), 2);
    assert_eq!(super::found(&expression, "rust:latest\n"), 0);
}
