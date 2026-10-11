//! A put killed while it writes its temporary file leaves the old version
//! of the record (docs/architecture.md, Writes).
#![cfg(test)]
#![expect(
    clippy::disallowed_methods,
    reason = "the test runs and kills a real board process, which only a child process can show, and opens a FIFO's write end so that a read waiting on it returns"
)]

use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};

use board_model::{canonical_json, parse_json};
use board_scenario::{Item, Scenario};

/// The put's temporary file, which the test makes a FIFO.
const TEMPORARY: &str = ".note.json.board-tmp";
/// A second name for that FIFO, which the put does not know, so the test
/// can still open the FIFO if the put removes the first name.
const SECOND_NAME: &str = ".git/kill-test-fifo";

/// A record of about a mebibyte whose text says which version it is.
fn record(version: usize) -> String {
    format!(
        r#"{{"$type": "lr/note/v1", "version": {version}, "text": "{}"}}"#,
        "x".repeat(1024 * 1024)
    )
}

fn canonical(text: &str) -> Vec<u8> {
    canonical_json(&parse_json(text.as_bytes()).unwrap()).into_bytes()
}

fn put(root: &Path, input: &str) -> Child {
    let mut child = Command::new(env!("CARGO_BIN_EXE_board"))
        .args(["put", "/note"])
        .current_dir(root)
        .env("XDG_CACHE_HOME", root.join(".git"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // A closed pipe only means the child exited before reading it all.
    let _ = child.stdin.take().unwrap().write_all(input.as_bytes());
    child
}

/// Kills the put while it writes to the FIFO named `fifo`. The put's write
/// blocks until the test reads from the FIFO, so the kill lands mid-write.
/// When the put exits without writing to it, returns the put's stderr.
fn kill_while_writing(child: &mut Child, fifo: &Path) -> Result<(), String> {
    let mut stdout = child.stdout.take().unwrap();
    std::thread::scope(|scope| {
        // The put's stdout closes when it exits. Opening the FIFO's write
        // end then frees a read that still waits for the put.
        let exited = scope.spawn(move || {
            let _ = std::io::copy(&mut stdout, &mut std::io::sink());
            drop(File::options().write(true).open(fifo).unwrap());
        });
        let mut reader = File::open(fifo).unwrap();
        let wrote = reader.read(&mut [0]).unwrap() == 1;
        let _ = child.kill();
        child.wait().unwrap();
        // The read end stays open until the write end above has opened.
        exited.join().unwrap();
        if wrote {
            return Ok(());
        }
        let mut stderr = String::new();
        child
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut stderr)
            .unwrap();
        Err(stderr)
    })
}

#[test]
fn a_put_killed_while_writing_its_temporary_file_leaves_the_old_version() {
    let scenario = Scenario::new();
    let old = canonical(&record(0));
    scenario
        .write(
            ".board/board.json",
            Item::File(r#"{"$type": "board/config/v1", "commit_on_put": false}"#),
        )
        .write(
            ".board/types/lr/note/v1.schema.json",
            Item::File(r#"{"type": "object"}"#),
        )
        .write("note.json", Item::Bytes(&old))
        .write(TEMPORARY, Item::Fifo);
    let root = scenario.workdir();
    std::fs::hard_link(root.join(TEMPORARY), root.join(SECOND_NAME)).unwrap();
    let mut child = put(root, &record(1));
    if let Err(stderr) = kill_while_writing(&mut child, &root.join(SECOND_NAME)) {
        panic!("the put exited without writing its temporary file; its stderr:\n{stderr}");
    }
    assert_eq!(std::fs::read(root.join("note.json")).unwrap(), old);
}
