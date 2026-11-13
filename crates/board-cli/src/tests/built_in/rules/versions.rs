//! A control with a version rule through the binary: `check` reports each
//! version a file pins that is older than the rule's, including files in a
//! mounted repository, and `evaluate` applies the rule to text.

use super::*;

/// A control that every `channel` its files pin is at least `at_least`.
fn floor(at_least: &str, paths: &[&str]) -> String {
    control(
        "Pin the latest Rust.",
        &json!({
            "paths": paths,
            "rule": {"check": "version", "at_least": at_least, "capture": r#"channel = "([0-9.]+)""#},
        }),
    )
}

fn toolchain(channel: &str) -> String {
    format!("[toolchain]\nchannel = \"{channel}\"\n")
}

#[test]
fn a_pin_older_than_the_rule_does_not_pass_and_one_at_the_rule_passes() {
    let board = board();
    board
        .write("app/rust-toolchain.toml", &toolchain("1.97.1"))
        .write(
            "controls/rust.json",
            &floor("1.99.1", &["*rust-toolchain.toml"]),
        );
    let output = board.call("rules.check", &[]);
    assert_eq!(output["passed"], false, "{output}");
    assert_eq!(
        output["versions"][0]["behind"],
        json!([{"file": "/app/rust-toolchain.toml", "line": 2, "found": "1.97.1"}])
    );
    board.write("app/rust-toolchain.toml", &toolchain("1.99.1"));
    let output = board.call("rules.check", &[]);
    assert_eq!(output["passed"], true, "{output}");
    assert_eq!(
        output["versions"],
        json!([{
            "rule": "/controls/rust",
            "requirement": "Pin the latest Rust.",
            "at_least": "1.99.1",
            "files": ["/app/rust-toolchain.toml"],
            "behind": [],
            "unreadable": [],
            "kind": "standing",
            "show": "failing",
        }])
    );
}

#[test]
fn a_mounted_repository_that_pins_an_older_version_does_not_pass_and_one_at_the_rule_passes() {
    let board = board();
    board.write(
        "controls/rust.json",
        &floor("1.99.1", &["repos/*/rust-toolchain.toml"]),
    );
    let _new = library(
        &board,
        "repos/new.json",
        &[("rust-toolchain.toml", &toolchain("1.99.1"))],
    );
    let output = board.call("rules.check", &["--path", "/repos/new"]);
    assert_eq!(
        output["versions"][0]["files"],
        json!(["/repos/new/rust-toolchain.toml"])
    );
    let _old = library(
        &board,
        "repos/old.json",
        &[("rust-toolchain.toml", &toolchain("1.98.0"))],
    );
    let output = board.call("rules.check", &["--path", "/repos/old"]);
    assert_eq!(output["passed"], false, "{output}");
    assert_eq!(
        output["versions"][0]["behind"],
        json!([{"file": "/repos/old/rust-toolchain.toml", "line": 2, "found": "1.98.0"}])
    );
}

#[test]
fn a_version_control_is_measured_and_never_stated() {
    let board = board();
    board.write(
        "controls/rust.json",
        &floor("1.99.1", &["*rust-toolchain.toml"]),
    );
    let output = board.call("rules.check", &[]);
    assert_eq!(output["stated"], json!([]));
    assert_eq!(output["versions"][0]["files"], json!([]));
}

#[test]
fn a_capture_without_a_group_fails_the_check_and_one_with_a_group_passes() {
    let board = board();
    let rule = |capture: &str| {
        control(
            "Pin the latest Rust.",
            &json!({"paths": ["*.toml"], "rule": {"check": "version", "at_least": "1.99.1", "capture": capture}}),
        )
    };
    board
        .write("rust-toolchain.toml", &toolchain("1.99.1"))
        .write("controls/rust.json", &rule("channel = .*"));
    let refused = board.refusal("rules.check", &[]);
    assert_eq!(refused["code"], "rules::capture::invalid", "{refused}");
    board.write("controls/rust.json", &rule("channel = \"(.*)\""));
    assert_eq!(
        board.call("rules.check", &[])["versions"][0]["behind"],
        json!([])
    );
}

#[test]
fn the_schema_refuses_an_at_least_that_is_not_a_dotted_number_and_admits_one_that_is() {
    let board = board();
    let rule = |at_least: &str| {
        control(
            "Pin the latest Rust.",
            &json!({"rule": {"check": "version", "at_least": at_least, "capture": "(.*)"}}),
        )
    };
    board
        .write("controls/latest.json", &rule("latest"))
        .write("controls/exact.json", &rule("1.99.1"));
    assert_eq!(
        board.checked("/controls"),
        codes(&[("/controls/exact", None), ("/controls/latest", INVALID)])
    );
}

#[test]
fn evaluate_reports_each_older_pin_in_a_part_and_passes_a_current_one() {
    let board = board();
    board.write("controls/rust.json", &floor("1.99.1", &[]));
    let input = json!({
        "rules": ["/controls/rust"],
        "parts": {"old": toolchain("1.97.1"), "new": toolchain("1.99.1")},
    });
    let ran = board.run_with(&["--json", "call", "rules.evaluate"], &input.to_string());
    let output = json(&ran)["data"]["output"].clone();
    let results = output["results"].as_array().unwrap();
    assert_eq!(output["passed"], false);
    assert_eq!(
        (results.iter())
            .map(|result| json!([result["part"], result["checker"], result["status"]]))
            .collect::<Vec<_>>(),
        [
            json!(["new", "version", "passed"]),
            json!(["old", "version", "failed"]),
        ]
    );
    assert_eq!(
        results[1]["findings"][0]["text"],
        "1.97.1, older than 1.99.1"
    );
    assert_eq!(results[1]["findings"][0]["line"], 2);
}
