//! A board's `min_board` makes this build refuse to write it, and not to
//! read it (docs/architecture/writes/minimum.md). The binary is a source
//! build numbered 0.0.0, so a board that asks for 1.0.0 is newer than it.
#![cfg(test)]
#![expect(
    clippy::disallowed_methods,
    reason = "only a child process shows what the board binary does with a board it is too old for"
)]

use std::io::Write;
use std::process::{Command, Output, Stdio};

use board_scenario::{Item, Scenario};

/// Runs `board` in `scenario`'s board with `args`, giving it `input` on
/// stdin when there is some and a closed stdin otherwise, so that no write
/// races a child that never reads.
fn run_with(
    scenario: &Scenario,
    home: &std::path::Path,
    args: &[&str],
    input: Option<&[u8]>,
) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_board"))
        .args(args)
        .current_dir(scenario.workdir())
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home)
        .env("XDG_CACHE_HOME", home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if let Some(input) = input {
        child.stdin.take().unwrap().write_all(input).unwrap();
    }
    child.wait_with_output().unwrap()
}

/// A command that reads no stdin.
fn run(scenario: &Scenario, home: &std::path::Path, args: &[&str]) -> Output {
    run_with(scenario, home, args, None)
}

/// A put, which reads its record from stdin.
fn put(scenario: &Scenario, home: &std::path::Path) -> Output {
    run_with(
        scenario,
        home,
        &["put", "/note", "--json"],
        Some(br#"{"$type": "lr/note/v1"}"#),
    )
}

fn board(min_board: &str) -> Scenario {
    let scenario = Scenario::board();
    scenario.write(
        ".board/board.json",
        Item::File(&format!(
            r#"{{"$type": "board/config/v1", "min_board": {min_board}}}"#
        )),
    );
    scenario.write(
        ".board/types/lr/note/v1.schema.json",
        Item::File(r#"{"type": "object"}"#),
    );
    scenario.identified();
    scenario
}

#[test]
fn a_build_older_than_min_board_is_refused_a_put_and_a_sync_but_reads() {
    let home = tempfile::tempdir().unwrap();
    let old = board(r#""1.0.0""#);
    let put = put(&old, home.path());
    assert_eq!(put.status.code(), Some(1));
    let error: serde_json::Value = serde_json::from_slice(&put.stdout).unwrap();
    assert_eq!(error["code"], "board::config::board_too_old");
    assert!(!old.workdir().join("note.json").exists());

    let sync = run(&old, home.path(), &["sync", "--json"]);
    let error: serde_json::Value = serde_json::from_slice(&sync.stdout).unwrap();
    assert_eq!(error["code"], "board::config::board_too_old");

    assert!(run(&old, home.path(), &["check"]).status.success());
}

#[test]
fn a_build_at_min_board_is_admitted_a_put() {
    let home = tempfile::tempdir().unwrap();
    let admitted = board(r#""0.0.0""#);
    let put = put(&admitted, home.path());
    assert_eq!(put.status.code(), Some(0), "{put:?}");
    assert!(admitted.workdir().join("note.json").exists());
}

/// The build date `board --version` prints, in seconds since the Unix epoch,
/// when it prints one.
fn version_date(scenario: &Scenario, home: &std::path::Path) -> Option<i64> {
    let output = run(scenario, home, &["--version"]);
    let text = String::from_utf8(output.stdout).unwrap();
    let (_, date) = text.split_once(" (built ")?;
    let date = date.trim_end().strip_suffix(')').unwrap();
    assert_eq!(date.len(), 20, "{text}");
    assert!(date.ends_with('Z') && date.as_bytes()[10] == b'T', "{text}");
    let at: jiff::Timestamp = date.parse().unwrap();
    Some(at.as_second())
}

/// Whether the source this test builds from is a git checkout, so that the
/// build had a commit date to embed.
fn in_a_checkout() -> bool {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.git")
        .exists()
}

#[test]
fn the_version_names_the_commit_date_of_the_source_when_the_build_has_one() {
    let home = tempfile::tempdir().unwrap();
    let scenario = board(r#""0.0.0""#);
    let date = version_date(&scenario, home.path());
    if in_a_checkout() && std::env::var_os("SOURCE_DATE_EPOCH").is_none() {
        assert!(date.is_some(), "a build in a git checkout has a date");
    }
}

#[test]
fn the_binary_gates_a_date_by_the_build_date_it_prints() {
    let home = tempfile::tempdir().unwrap();
    let Some(date) = version_date(&board(r#""0.0.0""#), home.path()) else {
        assert!(!in_a_checkout(), "a build in a git checkout has a date");
        return;
    };
    let after = |seconds: i64| {
        let at = jiff::Timestamp::from_second(seconds).unwrap();
        board(&format!("\"{at}\""))
    };
    // One second after the build's source was committed: this build is
    // older than the board allows.
    let refused = put(&after(date + 1), home.path());
    let error: serde_json::Value = serde_json::from_slice(&refused.stdout).unwrap();
    assert_eq!(error["code"], "board::config::board_too_old", "{error}");
    // At the date itself it is not.
    let admitted = put(&after(date), home.path());
    assert_eq!(admitted.status.code(), Some(0), "{admitted:?}");
}

#[test]
fn a_min_board_this_build_cannot_read_still_reads_and_refuses_a_write_naming_the_field() {
    let home = tempfile::tempdir().unwrap();
    let future = board(r#"{"version": "1.0.0", "commit": "abc"}"#);
    assert!(run(&future, home.path(), &["check"]).status.success());
    let put = put(&future, home.path());
    assert_eq!(put.status.code(), Some(1));
    let error: serde_json::Value = serde_json::from_slice(&put.stdout).unwrap();
    assert_eq!(
        error["code"], "board::config::min_board_unreadable",
        "{error}"
    );
    assert!(
        error["message"].as_str().unwrap().contains("`min_board`"),
        "{error}"
    );
}
