//! A put killed at any moment leaves the old file or the new one, never a
//! partial file (docs/architecture.md, Writes).
#![cfg(test)]
#![expect(
    clippy::disallowed_methods,
    reason = "the test kills a real board process, which only a child process can show"
)]

use std::io::Write;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use board_model::{canonical_json, parse_json};
use board_scenario::{Item, Scenario};

/// A record of about a mebibyte whose text says which version it is.
fn record(version: usize) -> String {
    format!(
        r#"{{"$type": "lr/note/v1", "version": {version}, "text": "{}"}}"#,
        "x".repeat(1024 * 1024)
    )
}

fn canonical(text: &str) -> Vec<u8> {
    canonical_json(&parse_json(text.as_bytes()).unwrap())
}

fn put(root: &Path, input: &str) -> Child {
    let mut child = Command::new(env!("CARGO_BIN_EXE_board"))
        .args(["put", "/note"])
        .current_dir(root)
        .env("XDG_CACHE_HOME", root.join(".git"))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    // A closed pipe only means the child was killed before reading it all.
    let _ = child.stdin.take().unwrap().write_all(input.as_bytes());
    child
}

/// Waits until the put starts writing, when its temporary file appears or
/// the target stops holding `old`, or until it exits.
fn writing(child: &mut Child, root: &Path, old: &[u8]) {
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(30) {
        if root.join(".note.json.board-tmp").exists()
            || std::fs::read(root.join("note.json")).is_ok_and(|now| now != old)
            || child.try_wait().unwrap().is_some()
        {
            return;
        }
    }
}

#[test]
fn a_put_killed_at_any_moment_leaves_the_old_file_or_the_new_one() {
    let scenario = Scenario::new();
    scenario
        .write(
            ".board/board.json",
            Item::File(r#"{"$type": "board/config/v1", "commit_on_put": false}"#),
        )
        .write(
            ".board/types/lr/note/v1.schema.json",
            Item::File(r#"{"type": "object"}"#),
        );
    let root = scenario.workdir();
    let mut versions = vec![canonical(&record(0))];
    scenario.write("note.json", Item::Bytes(&versions[0]));
    for attempt in 1..=6 {
        let old = std::fs::read(root.join("note.json")).unwrap();
        let input = record(attempt);
        versions.push(canonical(&input));
        let mut child = put(root, &input);
        // Half the kills land while the file is being written; the rest at
        // once, while the record is still being read and checked.
        if attempt % 2 == 0 {
            writing(&mut child, root, &old);
        }
        let _ = child.kill();
        child.wait().unwrap();
        let now = std::fs::read(root.join("note.json")).unwrap();
        assert!(
            versions.contains(&now),
            "attempt {attempt}: the file is neither an old version nor the new one ({} bytes)",
            now.len()
        );
        let _ = std::fs::remove_file(root.join(".note.json.board-tmp"));
        let _ = std::fs::remove_file(root.join(".git/board.lock"));
    }
}
