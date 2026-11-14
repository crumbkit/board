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

fn run(scenario: &Scenario, home: &std::path::Path, args: &[&str]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_board"))
        .args(args)
        .current_dir(scenario.workdir())
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home)
        .env("XDG_CACHE_HOME", home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(br#"{"$type": "lr/note/v1"}"#)
        .unwrap();
    child.wait_with_output().unwrap()
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
    let put = run(&old, home.path(), &["put", "/note", "--json"]);
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
    let put = run(&admitted, home.path(), &["put", "/note", "--json"]);
    assert_eq!(put.status.code(), Some(0), "{put:?}");
    assert!(admitted.workdir().join("note.json").exists());
}

#[test]
fn the_version_names_the_commit_date_of_the_source_when_the_build_has_one() {
    let home = tempfile::tempdir().unwrap();
    let scenario = board(r#""0.0.0""#);
    let output = run(&scenario, home.path(), &["--version"]);
    let text = String::from_utf8(output.stdout).unwrap();
    match text.split_once(" (built ") {
        Some((_, date)) => {
            let date = date.trim_end().strip_suffix(')').unwrap();
            assert_eq!(date.len(), 20, "{text}");
            assert!(date.ends_with('Z') && date.as_bytes()[10] == b'T', "{text}");
        }
        None => assert!(!text.contains("built"), "{text}"),
    }
}
