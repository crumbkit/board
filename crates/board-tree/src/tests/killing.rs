//! A put or a sync killed at each write point, then run again: the clone
//! ends as a clone that was never killed does
//! (docs/architecture/writes/sync/checkout.md). The test binary runs itself
//! as a child process, which aborts at the stop point the test names.
#![expect(
    clippy::disallowed_methods,
    reason = "the kill tests run the test binary as a child and name its stop point in the environment"
)]

use std::collections::BTreeMap;
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use super::*;
use crate::write::stop::Stop;
use crate::{Deadline, GIT_DEADLINE, LOCK_WAIT};

const STOP: &str = "BOARD_TEST_STOP";
const CLONE: &str = "BOARD_TEST_KILL_CLONE";
const ACTION: &str = "BOARD_TEST_KILL_ACTION";

/// The child process of every kill test. Run on its own, with no clone
/// named, it does nothing.
#[test]
fn the_child_of_the_kill_tests() {
    let Ok(clone) = std::env::var(CLONE) else {
        return;
    };
    let repository = Repository::open(Path::new(&clone)).unwrap();
    match std::env::var(ACTION).unwrap().as_str() {
        "sync" => {
            let _ = repository.sync(false, Deadline::after(GIT_DEADLINE), Duration::ZERO);
        }
        "put" => put(&repository, "b.json", b"{}\n"),
        other => panic!("no action {other}"),
    }
}

/// Writes `bytes` at `path` and commits them, as a put does.
fn put(repository: &Repository, path: &str, bytes: &[u8]) {
    let lock = repository.lock(LOCK_WAIT).unwrap();
    repository.write_file(&lock, path, bytes).unwrap();
    let staged = repository
        .stage_commit(&lock, &[(path, Some(bytes))], "board put")
        .unwrap()
        .unwrap();
    repository.finish_commit(&lock, staged).unwrap();
}

/// Runs `action` on `clone` in a child process that aborts at `stop`, and
/// checks that it did.
fn kill(clone: &Scenario, stop: Stop, action: &str) {
    let name = module_path!().split_once("::").unwrap().1;
    let status = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", &format!("{name}::the_child_of_the_kill_tests")])
        .env(STOP, stop.name())
        .env(CLONE, clone.workdir())
        .env(ACTION, action)
        .status()
        .unwrap();
    assert_eq!(status.signal(), Some(6), "{stop:?} did not abort: {status}");
}

fn sync(clone: &Scenario) -> Result<crate::Synced, crate::SyncError> {
    clone
        .open()
        .sync(false, Deadline::after(GIT_DEADLINE), Duration::ZERO)
}

/// Every file of the working tree outside `.git`, with its bytes.
fn working_tree(clone: &Scenario) -> BTreeMap<String, Vec<u8>> {
    fn walk(root: &Path, at: &Path, found: &mut BTreeMap<String, Vec<u8>>) {
        for entry in std::fs::read_dir(at).unwrap().map(Result::unwrap) {
            let path = entry.path();
            let name = path
                .strip_prefix(root)
                .unwrap()
                .to_str()
                .unwrap()
                .to_owned();
            if name == ".git" {
                continue;
            }
            if path.is_dir() {
                walk(root, &path, found);
            } else {
                found.insert(name, std::fs::read(&path).unwrap());
            }
        }
    }
    let mut found = BTreeMap::new();
    walk(clone.workdir(), clone.workdir(), &mut found);
    found
}

fn status(clone: &Scenario) -> String {
    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(clone.workdir())
        .output()
        .unwrap();
    String::from_utf8(output.stdout).unwrap()
}

/// What a kill and a rerun must leave in the clone, which a clone that was
/// never killed shows.
fn assert_same(killed: &Scenario, twin: &Scenario, case: &str) {
    assert_eq!(
        killed.head().unwrap().files,
        twin.head().unwrap().files,
        "{case}: HEAD's tree"
    );
    assert_eq!(
        working_tree(killed),
        working_tree(twin),
        "{case}: working tree"
    );
    assert_eq!(killed.index(), twin.index(), "{case}: index");
    assert_eq!(status(killed), "", "{case}: git status");
    for left in [
        "board.lock",
        "index.lock",
        "refs/heads/main.lock",
        "HEAD.lock",
        "board-locks",
        "board-checkout",
    ] {
        let path = killed.workdir().join(".git").join(left);
        assert!(!path.exists(), "{case}: {left} is left");
    }
}

const FIRST: [(&str, &str); 3] = [("a.json", "1"), ("b.json", "1"), ("c.json", "0")];

/// Where the remote goes after the killed sync, as the files it then holds.
const LATER: [(&str, [(&str, &str); 3]); 4] = [
    ("unchanged", FIRST),
    (
        "changed again at a file the checkout wrote",
        [("a.json", "2"), ("b.json", "1"), ("c.json", "0")],
    ),
    (
        "changed back at a file the checkout wrote",
        [("a.json", "0"), ("b.json", "1"), ("c.json", "0")],
    ),
    (
        "changed at another file",
        [("a.json", "1"), ("b.json", "1"), ("c.json", "2")],
    ),
];

/// A remote and two clones at its first commit, each able to commit.
fn setup() -> (Scenario, Scenario, Scenario) {
    let remote = Scenario::new();
    remote.identified().accepting_pushes();
    remote.commit(&[("a.json", "0"), ("b.json", "0"), ("c.json", "0")]);
    let killed = Scenario::clone_of(&remote);
    let twin = Scenario::clone_of(&remote);
    killed.identified();
    twin.identified();
    (remote, killed, twin)
}

#[test]
fn a_sync_killed_at_each_write_point_and_run_again_matches_one_that_was_never_killed() {
    for stop in Stop::ALL {
        for (later, files) in LATER {
            let case = format!("{stop:?}, remote {later}");
            let (remote, killed, twin) = setup();
            remote.commit(&FIRST);
            kill(&killed, stop, "sync");
            if files != FIRST {
                remote.commit(&files);
            }
            sync(&killed).unwrap_or_else(|error| panic!("{case}: {error}"));
            sync(&twin).unwrap();
            assert_same(&killed, &twin, &case);
        }
    }
}

#[test]
fn a_put_killed_while_it_holds_the_index_lock_leaves_the_next_put_able_to_run() {
    let scenario = Scenario::new();
    scenario.identified().commit(&[("a.json", "{}")]);
    scenario.write("a.json", Item::File("{}"));
    kill(&scenario, Stop::IndexLock, "put");
    let repository = scenario.open();
    put(&repository, "c.json", b"{}\n");
    let files = scenario.head().unwrap().files;
    assert!(files.contains_key("c.json"));
    for left in ["board.lock", "index.lock", "board-locks"] {
        assert!(
            !scenario.workdir().join(".git").join(left).exists(),
            "{left}"
        );
    }
}

#[test]
fn a_person_edit_to_a_file_a_killed_sync_wrote_still_refuses_the_next_sync() {
    let (remote, killed, _twin) = setup();
    remote.commit(&FIRST);
    kill(&killed, Stop::FirstFile, "sync");
    killed.write("a.json", Item::Fresh("mine"));
    remote.commit(&[("a.json", "2"), ("b.json", "1"), ("c.json", "0")]);
    let error = sync(&killed).unwrap_err();
    assert_eq!(error.code(), Code::SyncEdited, "{error}");
    assert_eq!(
        error.to_string(),
        "a.json is edited here and changed on the remote"
    );
    assert_eq!(working_tree(&killed)["a.json"], b"mine");
}

#[test]
fn two_syncs_killed_one_after_the_other_and_a_third_match_one_that_was_never_killed() {
    let (remote, killed, twin) = setup();
    remote.commit(&FIRST);
    kill(&killed, Stop::LastFile, "sync");
    remote.commit(&[("a.json", "2"), ("b.json", "0"), ("c.json", "0")]);
    kill(&killed, Stop::FirstFile, "sync");
    sync(&killed).unwrap();
    sync(&twin).unwrap();
    assert_same(&killed, &twin, "two kills");
}

/// The lock owner record, as the killed process left it.
fn owners(clone: &Scenario) -> String {
    std::fs::read_to_string(clone.workdir().join(".git/board-locks")).unwrap_or_default()
}

#[test]
fn a_lock_file_renamed_away_has_no_owner_entry_when_the_process_is_killed_after_it() {
    let (remote, killed, _twin) = setup();
    remote.commit(&FIRST);
    kill(&killed, Stop::IndexRenamed, "sync");
    let record = owners(&killed);
    assert!(!record.contains("index.lock"), "{record}");
    assert!(
        record.contains("main.lock"),
        "the branch lock is still held: {record}"
    );
    let (remote, killed, _twin) = setup();
    remote.commit(&FIRST);
    kill(&killed, Stop::BranchMoved, "sync");
    let record = owners(&killed);
    assert_eq!(
        record.lines().count(),
        1,
        "only board.lock is held: {record}"
    );
    assert!(record.contains("board.lock"), "{record}");
}
