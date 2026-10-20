//! `board sync` brings a clone up to date with the remote branch it
//! follows and pushes what it committed (docs/architecture/writes.md).

use super::*;

const PROJECT: &str = r#"{"$type": "lr/project/v1"}"#;

/// A remote holding the test board in one commit, accepting pushes, and
/// two clones of it with an identity to commit as.
pub(super) fn clones() -> (Scenario, Scenario, Scenario) {
    let remote = Scenario::new();
    remote.commit(&[
        (".board/board.json", CONFIG),
        (
            ".board/types/lr/project/v1.schema.json",
            r#"{"type": "object"}"#,
        ),
        ("projects/p.json", PROJECT),
    ]);
    remote.accepting_pushes();
    let clone = || {
        let clone = Scenario::clone_of(&remote);
        clone.write(
            ".git/config",
            Item::Appended("[user]\n\tname = Board Test\n\temail = board@example.com\n"),
        );
        clone
    };
    let (a, b) = (clone(), clone());
    (remote, a, b)
}

#[test]
fn sync_prints_what_it_fetched_replayed_and_pushed_and_where_head_moved() {
    let (remote, a, b) = clones();
    let put = |clone: &Scenario, at: &str| {
        let output = board_with(
            clone.workdir(),
            None,
            &["put", at, "--json"],
            PROJECT.as_bytes(),
        );
        json(&output)["data"]["commit"].as_str().unwrap().to_owned()
    };
    let base = remote.head().unwrap().id;
    let from_a = put(&a, "/projects/a");
    let from_b = put(&b, "/projects/b");
    assert_eq!(
        text(&board_in(a.workdir(), &["sync"]).stdout),
        format!("fetched origin/main {base}\npushed {from_a}\nHEAD is at {from_a} already\n")
    );
    let output = board_in(b.workdir(), &["sync", "--json"]);
    let data = &json(&output)["data"];
    let replayed = data["replayed"][0]["to"].as_str().unwrap().to_owned();
    assert_eq!(
        *data,
        json!({
            "remote": "origin",
            "branch": "main",
            "fetched": from_a,
            "old": from_b,
            "new": replayed,
            "replayed": [{"from": from_b, "to": replayed}],
            "pushed": true,
            "attempts": 1,
        })
    );
    assert_eq!(remote.head().unwrap().id.to_string(), replayed);
    let output = board_in(a.workdir(), &["sync"]);
    assert_eq!(
        text(&output.stdout),
        format!("fetched origin/main {replayed}\nmoved HEAD {from_a} -> {replayed}\n")
    );
    assert!(a.workdir().join("projects/b.json").exists());
}

#[test]
fn a_refused_push_ends_with_gits_reason_and_the_fetch_failure_carries_gits_stderr() {
    let (remote, a, _) = clones();
    remote.write(
        ".git/hooks/pre-receive",
        Item::Executable("#!/bin/sh\nexit 1\n"),
    );
    board_with(
        a.workdir(),
        None,
        &["put", "/projects/a"],
        PROJECT.as_bytes(),
    );
    let output = board_in(a.workdir(), &["sync"]);
    assert_eq!(output.code, 1);
    let stderr = text(&output.stderr);
    let lines: Vec<&str> = stderr.lines().collect();
    assert_eq!(
        lines[0],
        "error: the remote refused the push 3 times, the last time with [remote rejected] (pre-receive hook declined)"
    );
    assert_eq!(lines[2], "  code:  board::sync::gave_up", "{stderr}");
    assert!(lines[1].contains("`board sync --no-push`"), "{stderr}");
    a.unreachable_origin();
    let output = board_in(a.workdir(), &["sync", "--json"]);
    let failure = json(&output);
    assert_eq!(failure["code"], "board::sync::failed");
    assert!(
        failure["fields"]["stderr"]
            .as_str()
            .unwrap()
            .contains("/nowhere/at/all"),
        "{failure}"
    );
}
