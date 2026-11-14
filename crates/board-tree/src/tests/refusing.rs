//! The sync refusals that come before any write: a git merge or rebase in
//! progress, a shallow or partial clone, and a remote branch rewritten
//! under the clone (docs/architecture/writes.md, Sync).

use std::time::Duration;

use super::*;
use crate::{Deadline, GIT_DEADLINE, SyncError};

fn sync(clone: &Scenario, push: bool, accept_rewrite: bool) -> Result<crate::Synced, SyncError> {
    clone.open().sync_accepting(
        push,
        accept_rewrite,
        Deadline::after(GIT_DEADLINE),
        Duration::ZERO,
    )
}

/// A remote and a clone of it that can commit.
fn pair() -> (Scenario, Scenario) {
    let remote = Scenario::new();
    remote.commit(&[("a.json", "{}")]);
    remote.accepting_pushes();
    let clone = Scenario::clone_of(&remote);
    clone.identified();
    (remote, clone)
}

/// The remote-tracking ref's file.
fn tracking_ref(clone: &Scenario) -> String {
    std::fs::read_to_string(clone.workdir().join(".git/refs/remotes/origin/main")).unwrap()
}

/// What a refused sync must leave alone.
fn state(clone: &Scenario) -> (Option<ObjectId>, Vec<(String, ObjectId)>, Vec<u8>) {
    (
        clone.head().map(|head| head.id),
        clone.index().into_iter().collect(),
        std::fs::read(clone.workdir().join("a.json")).unwrap(),
    )
}

/// The remote's branch moved to a commit of another history.
fn rewrite(remote: &Scenario) -> ObjectId {
    remote.write(".git/refs/heads/main", Item::Deleted);
    remote.commit(&[("a.json", "{\"rewritten\": true}")])
}

#[test]
fn a_rewound_remote_refuses_leaves_the_clone_as_it_was_and_syncs_when_accepted() {
    let (remote, clone) = pair();
    let before = state(&clone);
    let tracking = tracking_ref(&clone);
    let rewritten = rewrite(&remote);
    let error = sync(&clone, true, false).unwrap_err();
    assert_eq!(error.code(), Code::SyncRewound, "{error}");
    assert!(
        error.to_string().contains(&rewritten.to_string()),
        "{error}"
    );
    assert_eq!(state(&clone), before);
    assert_eq!(tracking_ref(&clone), tracking);
    // The refusal does not wear off: the next run without the flag refuses too.
    let again = sync(&clone, true, false).unwrap_err();
    assert_eq!(again.code(), Code::SyncRewound);
    let synced = sync(&clone, false, true).unwrap();
    assert_eq!(synced.fetched, rewritten);
    assert!(synced.merged);
    assert_eq!(clone.head().unwrap().parents[1], rewritten);
}

#[test]
fn a_remote_that_only_moved_forward_syncs_without_the_flag() {
    let (remote, clone) = pair();
    let tip = remote.commit(&[("a.json", "{}"), ("b.json", "[]")]);
    assert_eq!(sync(&clone, false, false).unwrap().new, tip);
}

#[test]
fn a_remote_moved_back_to_an_ancestor_refuses_as_rewound() {
    let (remote, clone) = pair();
    let first = remote.head().unwrap().id;
    remote.commit(&[("a.json", "{}"), ("b.json", "[]")]);
    assert_eq!(
        sync(&clone, false, false)
            .unwrap()
            .fetched
            .to_string()
            .len(),
        40
    );
    remote.write(".git/refs/heads/main", Item::File(&format!("{first}\n")));
    let error = sync(&clone, false, false).unwrap_err();
    assert_eq!(error.code(), Code::SyncRewound, "{error}");
}

#[test]
fn a_clone_with_no_tracking_ref_is_not_refused_as_rewound() {
    let (remote, clone) = pair();
    clone.write(".git/refs/remotes/origin/main", Item::Deleted);
    let tip = remote.commit(&[("a.json", "{}"), ("b.json", "[]")]);
    assert_eq!(sync(&clone, false, false).unwrap().new, tip);
}

#[test]
fn a_shallow_clone_refuses_even_when_a_rewrite_is_accepted_and_a_full_twin_syncs() {
    let (remote, clone) = pair();
    let tip = remote.commit(&[("a.json", "{}"), ("b.json", "[]")]);
    let before = state(&clone);
    clone.write(
        ".git/shallow",
        Item::File(&format!("{}\n", clone.head().unwrap().id)),
    );
    for accept in [false, true] {
        let error = sync(&clone, false, accept).unwrap_err();
        assert_eq!(error.code(), Code::SyncShallow, "{error}");
        assert_eq!(state(&clone), before);
    }
    clone.write(".git/shallow", Item::Deleted);
    assert_eq!(sync(&clone, false, false).unwrap().new, tip);
}

#[test]
fn a_partial_clone_refuses_as_shallow() {
    let (remote, clone) = pair();
    remote.commit(&[("a.json", "{}"), ("b.json", "[]")]);
    clone.write(
        ".git/config",
        Item::Appended("[remote \"origin\"]\n\tpromisor = true\n"),
    );
    let error = sync(&clone, false, false).unwrap_err();
    assert_eq!(error.code(), Code::SyncShallow, "{error}");
    let before = state(&clone);
    assert_eq!(state(&clone), before);
}

#[test]
fn a_merge_or_rebase_in_progress_refuses_even_when_a_rewrite_is_accepted_and_the_twin_syncs() {
    for marker in [".git/MERGE_HEAD", ".git/rebase-merge", ".git/rebase-apply"] {
        let (remote, clone) = pair();
        let tip = remote.commit(&[("a.json", "{}"), ("b.json", "[]")]);
        let before = state(&clone);
        clone.write(marker, Item::File("x\n"));
        for accept in [false, true] {
            let error = sync(&clone, false, accept).unwrap_err();
            assert_eq!(error.code(), Code::SyncMerging, "{marker}: {error}");
            assert_eq!(state(&clone), before);
        }
        clone.write(marker, Item::Deleted);
        assert_eq!(sync(&clone, false, false).unwrap().new, tip, "{marker}");
    }
}

#[test]
fn the_refusals_come_before_the_board_lock_is_taken() {
    let (remote, clone) = pair();
    rewrite(&remote);
    let repository = clone.open();
    let _held = repository.lock(crate::LOCK_WAIT).unwrap();
    let error = sync(&clone, false, false).unwrap_err();
    assert_eq!(error.code(), Code::SyncRewound, "{error}");
}
