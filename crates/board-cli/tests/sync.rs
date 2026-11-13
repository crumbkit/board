//! Ctrl-C on `board sync` stops git and everything git started, and leaves
//! the board as it was (docs/architecture/writes.md).
#![cfg(test)]
#![expect(
    clippy::disallowed_methods,
    reason = "the test signals a real board process, which only a child process can show"
)]

use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use board_scenario::{Item, Scenario};

/// A stand-in for ssh that writes git's id, its own and that of the `sleep`
/// it starts, then waits on the `sleep`, as a hung remote would.
const STALLING: &str = "#!/bin/sh\necho $PPID > \"$0.pid\"\necho $$ >> \"$0.pid\"\n\
                        sleep 30 &\necho $! >> \"$0.pid\"\nwait\n";

/// A clone of a remote that has moved on, whose origin stalls.
fn stalling() -> Scenario {
    let remote = Scenario::new();
    remote.commit(&[(".board/board.json", r#"{"$type": "board/config/v1"}"#)]);
    let clone = Scenario::clone_of(&remote);
    remote.commit(&[
        (".board/board.json", r#"{"$type": "board/config/v1"}"#),
        ("a.md", "new"),
    ]);
    clone.delayed_origin(30);
    clone.write(".git/delayed-ssh", Item::Executable(STALLING));
    clone
}

/// Starts `board sync` in `clone`, leading a new process group as a shell
/// starts a job, so Ctrl-C can be sent to that group.
fn sync(clone: &Scenario, cache: &Path) -> Child {
    Command::new(env!("CARGO_BIN_EXE_board"))
        .process_group(0)
        .arg("sync")
        .current_dir(clone.workdir())
        .env("XDG_CACHE_HOME", cache)
        .env("XDG_CONFIG_HOME", cache.join("config"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

/// The ids the stand-in for ssh wrote: git's, its own and the `sleep`'s.
fn written(clone: &Scenario, board: &mut Child) -> Vec<String> {
    let path = clone.workdir().join(".git/delayed-ssh.pid");
    loop {
        let exited = board.try_wait().unwrap();
        if let Ok(text) = std::fs::read_to_string(&path) {
            let ids: Vec<String> = text.lines().map(str::to_owned).collect();
            if ids.len() == 3 && text.ends_with('\n') {
                return ids;
            }
        }
        assert!(
            exited.is_none(),
            "board exited before git reached the remote"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Whether the process `pid` has ended within a bounded number of checks.
fn ends(pid: &str) -> bool {
    (0..500).any(|_| {
        let output = Command::new("ps")
            .args(["-o", "stat=", "-p", pid])
            .output()
            .unwrap();
        let state = String::from_utf8_lossy(&output.stdout);
        // A zombie has ended; it only waits for init to reap it.
        let ended = state.trim().is_empty() || state.trim_start().starts_with('Z');
        if !ended {
            std::thread::sleep(Duration::from_millis(10));
        }
        ended
    })
}

/// Every file under `.git` whose name ends in `.lock`.
fn locks(directory: &Path) -> Vec<String> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            found.extend(locks(&path));
        } else if path
            .extension()
            .is_some_and(|extension| extension == "lock")
        {
            found.push(path.display().to_string());
        }
    }
    found
}

#[test]
fn ctrl_c_on_a_stalled_sync_stops_git_and_what_it_started_and_leaves_the_board_as_it_was() {
    let clone = stalling();
    let cache = tempfile::tempdir().unwrap();
    let before = clone.head().unwrap().id;
    let tracking =
        std::fs::read_to_string(clone.workdir().join(".git/refs/remotes/origin/main")).unwrap();
    let mut board = sync(&clone, cache.path());
    let ids = written(&clone, &mut board);
    let group = format!("-{}", board.id());
    let sent = Command::new("kill")
        .args(["-s", "INT", "--", &group])
        .status()
        .unwrap();
    assert!(sent.success());
    let output = board.wait_with_output().unwrap();
    assert_eq!(
        output.status.signal(),
        Some(2),
        "board stops on SIGINT: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    for (name, id) in ["git", "the stand-in for ssh", "its sleep"]
        .iter()
        .zip(&ids)
    {
        assert!(ends(id), "{name} ({id}) is still running");
    }
    assert_eq!(clone.head().unwrap().id, before);
    assert_eq!(
        std::fs::read_to_string(clone.workdir().join(".git/refs/remotes/origin/main")).unwrap(),
        tracking
    );
    assert!(!clone.workdir().join("a.md").exists());
    // sync.lock stays as a file; the kernel dropped its lock with the process.
    let mut left = locks(&clone.workdir().join(".git"));
    left.retain(|lock| !lock.ends_with("/sync.lock"));
    assert_eq!(left, Vec::<String>::new());
    drop(clone.flocked(".git/sync.lock"));
}
