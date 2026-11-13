//! Committing what a put wrote (docs/architecture.md, Writes).

use super::*;
use crate::{LOCK_WAIT, WriteError};
use std::time::{Duration, Instant};

fn identified() -> Scenario {
    let scenario = Scenario::new();
    scenario.identified();
    scenario.commit(&[("a.json", "{}")]);
    scenario
}

#[test]
fn head_moved_since_staging_leaves_the_commit_unmade_and_the_index_alone() {
    let scenario = identified();
    let repository = scenario.open();
    let lock = repository.lock(LOCK_WAIT).unwrap();
    let staged = repository
        .stage_commit(&lock, &[("b.json", Some(b"{}\n"))], "board put /b")
        .unwrap()
        .unwrap();
    let moved = scenario.commit(&[("a.json", "{}"), ("c.json", "{}")]);
    assert!(matches!(
        repository.finish_commit(&lock, staged),
        Err(WriteError::HeadMoved)
    ));
    assert_eq!(scenario.head().unwrap().id, moved);
    assert!(!scenario.index().contains_key("b.json"));
    assert!(
        !repository.git_dir().join("index.lock").exists(),
        "git's index lock is released"
    );
}

#[test]
fn head_unmoved_since_staging_takes_the_commit_and_the_index_follows() {
    let scenario = identified();
    let repository = scenario.open();
    let parent = scenario.head().unwrap().id;
    let lock = repository.lock(LOCK_WAIT).unwrap();
    let staged = repository
        .stage_commit(&lock, &[("b.json", Some(b"{}\n"))], "board put /b")
        .unwrap()
        .unwrap();
    let commit = repository.finish_commit(&lock, staged).unwrap();
    let head = scenario.head().unwrap();
    assert_eq!((head.id, head.parents.as_slice()), (commit, &[parent][..]));
    assert_eq!(head.files["b.json"], b"{}\n");
    assert_eq!(head.files["a.json"], b"{}", "the rest of HEAD is kept");
    assert_eq!(scenario.index()["b.json"], scenario.blob("{}\n"));
}

#[test]
fn a_removal_leaves_the_path_out_of_the_commit_and_the_index() {
    let scenario = identified();
    let repository = scenario.open();
    let lock = repository.lock(LOCK_WAIT).unwrap();
    for bytes in [Some(&b"{}\n"[..]), None] {
        let staged = repository
            .stage_commit(&lock, &[("b.json", bytes)], "board put")
            .unwrap()
            .unwrap();
        repository.finish_commit(&lock, staged).unwrap();
        assert_eq!(scenario.index().contains_key("b.json"), bytes.is_some());
    }
    let head = scenario.head().unwrap();
    assert!(!head.files.contains_key("b.json"));
    assert_eq!(head.files["a.json"], b"{}", "the rest of HEAD is kept");
}

#[test]
fn a_detached_head_takes_the_commit_on_itself() {
    let scenario = identified();
    let parent = scenario.head().unwrap().id;
    scenario.write(".git/HEAD", Item::File(&format!("{parent}\n")));
    let repository = scenario.open();
    let lock = repository.lock(LOCK_WAIT).unwrap();
    let staged = repository
        .stage_commit(&lock, &[("b.json", Some(b"{}\n"))], "board put /b")
        .unwrap()
        .unwrap();
    let commit = repository.finish_commit(&lock, staged).unwrap();
    assert_eq!(
        std::fs::read_to_string(repository.git_dir().join("HEAD"))
            .unwrap()
            .trim(),
        commit.to_string()
    );
}

/// The ref HEAD names, such as `refs/heads/main`.
fn branch(scenario: &Scenario) -> String {
    let head = std::fs::read_to_string(scenario.workdir().join(".git/HEAD")).unwrap();
    head.trim().strip_prefix("ref: ").unwrap().to_owned()
}

#[test]
fn a_held_branch_lock_is_a_failure_and_not_head_moved() {
    let scenario = identified();
    let lock_file = format!(".git/{}.lock", branch(&scenario));
    scenario.write(&lock_file, Item::File(""));
    let repository = scenario.open();
    let parent = scenario.head().unwrap().id;
    let lock = repository.lock(LOCK_WAIT).unwrap();
    let staged = repository
        .stage_commit(&lock, &[("b.json", Some(b"{}\n"))], "board put /b")
        .unwrap()
        .unwrap();
    let error = repository.finish_commit(&lock, staged).unwrap_err();
    assert!(matches!(error, WriteError::Uncommitted { .. }), "{error}");
    assert_eq!(error.code(), Code::PutFailed);
    assert!(error.to_string().contains("written but not committed"));
    assert_eq!(scenario.head().unwrap().id, parent);
    assert!(!repository.git_dir().join("index.lock").exists());
    scenario.write(&lock_file, Item::Deleted);
    let staged = repository
        .stage_commit(&lock, &[("b.json", Some(b"{}\n"))], "board put /b")
        .unwrap()
        .unwrap();
    assert!(repository.finish_commit(&lock, staged).is_ok());
}

/// Git reads HEAD and then the index, so a HEAD that moves before the index
/// does lets a commit made at that moment take a tree without the put's files.
/// The index is therefore replaced first, while the branch is locked.
#[test]
fn an_index_that_cannot_be_written_leaves_head_where_it_was() {
    let scenario = identified();
    let repository = scenario.open();
    let parent = scenario.head().unwrap().id;
    let lock = repository.lock(LOCK_WAIT).unwrap();
    let staged = repository
        .stage_commit(&lock, &[("b.json", Some(b"{}\n"))], "board put /b")
        .unwrap()
        .unwrap();
    // The index is written into the held lock file, so with it gone the
    // write fails.
    scenario.write(".git/index.lock", Item::Deleted);
    let error = repository.finish_commit(&lock, staged).unwrap_err();
    assert!(matches!(error, WriteError::IndexNotWritten { .. }), "{error}");
    assert_eq!(error.code(), Code::PutFailed);
    assert!(
        error
            .to_string()
            .starts_with("git's index could not be written, so the files are written but not committed")
    );
    assert_eq!(scenario.head().unwrap().id, parent, "HEAD did not move");
    assert!(!scenario.index().contains_key("b.json"));
    let branch_lock = format!(".git/{}.lock", branch(&scenario));
    assert!(
        !scenario.workdir().join(branch_lock).exists(),
        "the branch lock is released"
    );
}

#[test]
fn a_commit_during_a_merge_or_rebase_is_refused_and_a_clean_one_is_admitted() {
    for (marker, operation) in [
        ("MERGE_HEAD", "merge"),
        ("CHERRY_PICK_HEAD", "cherry-pick"),
        ("REVERT_HEAD", "revert"),
        ("rebase-merge/head-name", "rebase"),
        ("rebase-apply/next", "rebase or am"),
    ] {
        let scenario = identified();
        let parent = scenario.head().unwrap().id;
        scenario.write(
            &format!(".git/{marker}"),
            Item::File(&format!("{parent}\n")),
        );
        let repository = scenario.open();
        let lock = repository.lock(LOCK_WAIT).unwrap();
        let error = repository
            .stage_commit(&lock, &[("b.json", Some(b"{}\n"))], "board put /b")
            .unwrap_err();
        assert_eq!(error.code(), Code::PutLocked, "{marker}");
        assert_eq!(
            error.to_string(),
            format!(
                "git is in the middle of a {operation}; finish or abort it before a put commits"
            )
        );
        assert!(!repository.git_dir().join("index.lock").exists());
        let top = marker.split('/').next().unwrap();
        scenario.write(&format!(".git/{top}"), Item::Deleted);
        assert!(
            repository
                .stage_commit(&lock, &[("b.json", Some(b"{}\n"))], "board put /b")
                .is_ok(),
            "{marker}"
        );
    }
}

/// An index that cannot be read stands in for one another writer is
/// rewriting: a commit that read it before taking the lock fails on it.
#[test]
fn a_held_index_lock_refuses_a_commit_before_the_index_is_read_and_a_free_one_is_admitted() {
    let scenario = identified();
    scenario
        .write(".git/index", Item::File("not an index"))
        .write(".git/index.lock", Item::File(""));
    let repository = scenario.open();
    let lock = repository.lock(Duration::from_millis(100)).unwrap();
    let error = repository
        .stage_commit(&lock, &[("b.json", Some(b"{}\n"))], "board put /b")
        .unwrap_err();
    assert_eq!(error.code(), Code::PutLocked, "{error}");
    let held = repository.git_dir().join("index.lock");
    assert_eq!(
        error.to_string(),
        format!("{held:?} is held; if no board process is running, remove it")
    );
    scenario
        .write(".git/index", Item::Deleted)
        .write(".git/index.lock", Item::Deleted);
    assert!(
        repository
            .stage_commit(&lock, &[("b.json", Some(b"{}\n"))], "board put /b")
            .is_ok()
    );
}

/// A lock another process drops while the put waits is taken, within the
/// same wait that `board.lock` has.
#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "another process drops git's lock, which the scenario cannot do from a thread"
)]
fn an_index_lock_released_after_a_second_admits_the_commit() {
    let scenario = identified();
    scenario.write(".git/index.lock", Item::File(""));
    let repository = scenario.open();
    let held = repository.git_dir().join("index.lock");
    let lock = repository.lock(LOCK_WAIT).unwrap();
    let releaser = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(1));
        std::fs::remove_file(held).unwrap();
    });
    let started = Instant::now();
    let staged = repository
        .stage_commit(&lock, &[("b.json", Some(b"{}\n"))], "board put /b")
        .unwrap();
    releaser.join().unwrap();
    assert!(staged.is_some());
    assert!(started.elapsed() >= Duration::from_millis(900));
}

/// The twin of the test above: the lock outlasts the wait, so the put is
/// refused naming the file, and nothing is written.
#[test]
fn an_index_lock_held_past_the_wait_refuses_naming_the_file_and_writes_nothing() {
    let scenario = identified();
    scenario.write(".git/index.lock", Item::File(""));
    let repository = scenario.open();
    let parent = scenario.head().unwrap().id;
    let lock = repository.lock(Duration::from_millis(300)).unwrap();
    let started = Instant::now();
    let error = repository
        .stage_commit(&lock, &[("b.json", Some(b"{}\n"))], "board put /b")
        .unwrap_err();
    assert!(started.elapsed() >= Duration::from_millis(250));
    assert_eq!(error.code(), Code::PutLocked, "{error}");
    assert!(error.to_string().contains("index.lock"), "{error}");
    assert_eq!(scenario.head().unwrap().id, parent);
    assert!(!scenario.index().contains_key("b.json"));
}

/// A put whose HEAD moved to a commit that leaves its paths alone stages
/// again on the new HEAD and commits.
#[test]
fn an_unrelated_commit_under_a_put_is_admitted_on_the_new_head() {
    let scenario = identified();
    let repository = scenario.open();
    let lock = repository.lock(LOCK_WAIT).unwrap();
    let writes: &[(&str, Option<&[u8]>)] = &[("b.json", Some(b"{}\n"))];
    let seen = scenario.head().map(|head| head.id);
    let staged = repository
        .stage_commit(&lock, writes, "board put /b")
        .unwrap()
        .unwrap();
    let moved = scenario.commit(&[("a.json", "{}"), ("c.json", "{}")]);
    let commit = repository
        .finish_commit_retrying(&lock, staged, seen, writes, "board put /b", &|_| {})
        .unwrap()
        .unwrap();
    let head = scenario.head().unwrap();
    assert_eq!((head.id, head.parents.as_slice()), (commit, &[moved][..]));
    assert_eq!(head.files["b.json"], b"{}\n");
    assert_eq!(head.files["c.json"], b"{}", "the other commit is kept");
    assert_eq!(scenario.index()["b.json"], scenario.blob("{}\n"));
}

/// The twin: the other commit wrote one of the put's paths, so the put
/// would commit over a change its caller never saw.
#[test]
fn a_commit_to_a_written_path_under_a_put_refuses_head_moved() {
    let scenario = identified();
    let repository = scenario.open();
    let lock = repository.lock(LOCK_WAIT).unwrap();
    let writes: &[(&str, Option<&[u8]>)] = &[("a.json", Some(b"{\"n\":1}\n"))];
    let seen = scenario.head().map(|head| head.id);
    let staged = repository
        .stage_commit(&lock, writes, "board put /a")
        .unwrap()
        .unwrap();
    let moved = scenario.commit(&[("a.json", "{\"n\":2}")]);
    let error = repository
        .finish_commit_retrying(&lock, staged, seen, writes, "board put /a", &|_| {})
        .unwrap_err();
    assert!(matches!(error, WriteError::HeadMoved), "{error}");
    assert_eq!(scenario.head().unwrap().id, moved);
    assert!(!repository.git_dir().join("index.lock").exists());
}
