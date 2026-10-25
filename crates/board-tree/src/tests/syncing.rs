//! Syncing below a board: a clone with no commit yet, and git's answer to
//! a push (docs/architecture.md, Writes).

use super::*;
use crate::transport::rejected;
use std::time::{Duration, Instant};

use crate::{GIT_DEADLINE, LOCK_WAIT, Synced};

#[test]
fn a_clone_without_a_commit_checks_out_the_remote_branch_without_pushing() {
    let remote = Scenario::new();
    let tip = remote.commit(&[("a.json", "{}"), ("d/b.json", "[]")]);
    let clone = Scenario::clone_of(&remote);
    clone.write(".git/refs/heads/main", Item::Deleted);
    clone.write(".git/index", Item::Deleted);
    clone.write("a.json", Item::Deleted);
    clone.write("d/b.json", Item::Deleted);
    let repository = clone.open();
    let lock = repository.lock(LOCK_WAIT).unwrap();
    let synced = repository.sync(&lock, true, GIT_DEADLINE).unwrap();
    assert_eq!(
        synced,
        Synced {
            upstream: repository.upstream().unwrap(),
            fetched: tip,
            old: None,
            new: tip,
            replayed: Vec::new(),
            pushed: false,
            attempts: 1,
        }
    );
    assert_eq!(clone.head().unwrap().id, tip);
    assert_eq!(clone.index().len(), 2);
    assert_eq!(
        std::fs::read_to_string(clone.workdir().join("d/b.json")).unwrap(),
        "[]"
    );
}

#[test]
fn a_push_git_reports_as_rejected_names_the_reason_and_a_failed_one_has_none() {
    let refused = b"To /remote\n!\tabc123:refs/heads/main\t[rejected] (fetch first)\nDone\n";
    assert_eq!(
        rejected(refused),
        Some("[rejected] (fetch first)".to_owned())
    );
    assert_eq!(rejected(b"fatal: could not read from remote\n"), None);
}

/// A sync removes a file the remote deleted, is done when it is already
/// gone, and names the path it cannot remove.
#[test]
fn removing_a_file_is_done_when_it_is_gone_and_names_a_path_it_cannot_remove() {
    let scenario = Scenario::new();
    scenario.write("a.json", Item::File("{}"));
    scenario.write("d/b.json", Item::File("[]"));
    let repository = scenario.open();
    let lock = repository.lock(LOCK_WAIT).unwrap();
    repository.remove_file(&lock, "a.json").unwrap();
    assert!(!scenario.workdir().join("a.json").exists());
    repository.remove_file(&lock, "a.json").unwrap();
    let refused = repository.remove_file(&lock, "d").unwrap_err();
    assert!(
        matches!(&refused, crate::WriteError::Io { path, .. } if path.ends_with("d")),
        "{refused:?}"
    );
    assert!(scenario.workdir().join("d/b.json").exists());
}

/// A file the remote deleted leaves the working tree and the index, and
/// the other files stay in both.
#[test]
fn a_sync_removes_a_file_the_remote_deleted_and_keeps_the_rest() {
    let remote = Scenario::new();
    remote.commit(&[("a.json", "{}"), ("d/b.json", "[]")]);
    let clone = Scenario::clone_of(&remote);
    let tip = remote.commit(&[("d/b.json", "[]")]);
    let repository = clone.open();
    let lock = repository.lock(LOCK_WAIT).unwrap();
    assert_eq!(
        repository.sync(&lock, false, GIT_DEADLINE).unwrap().new,
        tip
    );
    assert!(!clone.workdir().join("a.json").exists());
    assert!(clone.workdir().join("d/b.json").exists());
    assert_eq!(
        clone.index().into_keys().collect::<Vec<_>>(),
        vec!["d/b.json".to_owned()]
    );
}

/// A path the sync must check but cannot read stops it, naming the path.
#[test]
fn a_sync_stops_at_a_path_it_cannot_read() {
    let remote = Scenario::new();
    remote.commit(&[("a.json", "{}")]);
    let clone = Scenario::clone_of(&remote);
    remote.commit(&[("a.json", "{}"), ("d", "x")]);
    clone.write("d/x.json", Item::File("{}"));
    let repository = clone.open();
    let lock = repository.lock(LOCK_WAIT).unwrap();
    let refused = repository.sync(&lock, false, GIT_DEADLINE).unwrap_err();
    assert!(
        matches!(&refused, crate::SyncError::Git(crate::git::GitError::File { path, .. }) if path.ends_with("d")),
        "{refused:?}"
    );
    assert!(clone.workdir().join("d/x.json").exists());
}

/// A sync that a write refuses names the path the write names.
#[test]
fn a_sync_a_write_refuses_names_the_writes_path() {
    let path: LogicalPath = "/x/y".parse().unwrap();
    let refused = crate::SyncError::Write(WriteError::Symlink {
        path: path.clone(),
        symlink: "x".to_owned(),
    });
    assert_eq!(refused.fields(), board_model::Fields::at(&path));
}

/// A remote that has not answered when the deadline passes stops the sync
/// with `board::sync::timeout`: Board stops git and returns soon after the
/// deadline, so the caller lets the lock go. The same remote answering
/// within the deadline syncs.
#[test]
fn a_remote_that_stalls_past_the_deadline_times_out_and_one_that_answers_syncs() {
    let deadline = Duration::from_secs(5);
    let sync = |delay| {
        let remote = Scenario::new();
        remote.commit(&[("a.json", "{}")]);
        let clone = Scenario::clone_of(&remote);
        let tip = remote.commit(&[("a.json", "[]")]);
        clone.delayed_origin(delay);
        let repository = clone.open();
        let lock = repository.lock(LOCK_WAIT).unwrap();
        let started = Instant::now();
        let synced = repository.sync(&lock, false, deadline);
        let took = started.elapsed();
        (synced, took, tip)
    };
    let (refused, took, _) = sync(30);
    let refused = refused.unwrap_err();
    assert_eq!(refused.code(), board_model::Code::SyncTimeout, "{refused}");
    assert!(took < Duration::from_secs(10), "{took:?}");
    let (admitted, _, tip) = sync(0);
    assert_eq!(admitted.unwrap().new, tip);
}
