//! A committing put needs git's user.name and user.email; without them it is
//! refused before it writes (docs/architecture.md, Writes). The process runs
//! with an empty home and no system configuration, so the machine's own git
//! identity cannot answer for it.
#![cfg(test)]
#![expect(
    clippy::disallowed_methods,
    reason = "only a child process can be given an environment without a git identity"
)]

use std::io::Write;
use std::process::{Command, Output, Stdio};

use board_scenario::{Item, Scenario};

fn put(scenario: &Scenario, home: &std::path::Path) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_board"))
        .args(["put", "/note", "--json"])
        .current_dir(scenario.workdir())
        .env_clear()
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

fn board() -> Scenario {
    let scenario = Scenario::new();
    scenario
        .write(
            ".board/board.json",
            Item::File(r#"{"$type": "board/config/v1"}"#),
        )
        .write(
            ".board/types/lr/note/v1.schema.json",
            Item::File(r#"{"type": "object"}"#),
        );
    scenario
}

#[test]
fn a_committing_put_without_an_identity_is_refused_before_writing_and_with_one_is_committed() {
    let home = tempfile::tempdir().unwrap();
    let anonymous = board();
    let refused = put(&anonymous, home.path());
    assert_eq!(refused.status.code(), Some(1));
    let error: serde_json::Value = serde_json::from_slice(&refused.stdout).unwrap();
    assert_eq!(error["code"], "board::put::no_identity");
    assert!(!anonymous.workdir().join("note.json").exists());

    let known = board();
    known.write(
        ".git/config",
        Item::Appended("[user]\n\tname = Board Test\n\temail = board@example.com\n"),
    );
    let admitted = put(&known, home.path());
    assert_eq!(
        admitted.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&admitted.stderr)
    );
    assert!(known.workdir().join("note.json").exists());
    assert_eq!(known.head().unwrap().message, "board put /note");
}
