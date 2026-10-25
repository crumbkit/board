//! Committing what a put wrote (docs/architecture.md, Writes).

use super::*;
use crate::{LOCK_WAIT, WriteError};

fn identified() -> Scenario {
    let scenario = Scenario::new();
    scenario.write(
        ".git/config",
        Item::Appended("[user]\n\tname = Board Test\n\temail = board@example.com\n"),
    );
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

#[test]
fn an_index_that_cannot_be_written_after_head_moved_names_the_commit() {
    let scenario = identified();
    let repository = scenario.open();
    let lock = repository.lock(LOCK_WAIT).unwrap();
    let staged = repository
        .stage_commit(&lock, &[("b.json", Some(b"{}\n"))], "board put /b")
        .unwrap()
        .unwrap();
    // The index is written into the held lock file, so with it gone the
    // write fails after HEAD has moved.
    scenario.write(".git/index.lock", Item::Deleted);
    let error = repository.finish_commit(&lock, staged).unwrap_err();
    let head = scenario.head().unwrap().id;
    assert!(
        matches!(error, WriteError::IndexNotUpdated { commit, .. } if commit == head),
        "{error}"
    );
    assert_eq!(error.code(), Code::PutFailed);
    assert!(error.to_string().starts_with(&format!(
        "committed {head}, but git's index was not updated"
    )));
    assert!(!scenario.index().contains_key("b.json"));
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
