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
        .stage_commit(&lock, &[("b.json", b"{}\n")], "board put /b")
        .unwrap();
    let moved = scenario.commit(&[("a.json", "{}"), ("c.json", "{}")]);
    assert!(matches!(
        repository.finish_commit(staged),
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
        .stage_commit(&lock, &[("b.json", b"{}\n")], "board put /b")
        .unwrap();
    let commit = repository.finish_commit(staged).unwrap();
    let head = scenario.head().unwrap();
    assert_eq!((head.id, head.parents.as_slice()), (commit, &[parent][..]));
    assert_eq!(head.files["b.json"], b"{}\n");
    assert_eq!(head.files["a.json"], b"{}", "the rest of HEAD is kept");
    assert_eq!(scenario.index()["b.json"], scenario.blob("{}\n"));
}

#[test]
fn a_detached_head_takes_the_commit_on_itself() {
    let scenario = identified();
    let parent = scenario.head().unwrap().id;
    scenario.write(".git/HEAD", Item::File(&format!("{parent}\n")));
    let repository = scenario.open();
    let lock = repository.lock(LOCK_WAIT).unwrap();
    let staged = repository
        .stage_commit(&lock, &[("b.json", b"{}\n")], "board put /b")
        .unwrap();
    let commit = repository.finish_commit(staged).unwrap();
    assert_eq!(
        std::fs::read_to_string(repository.git_dir().join("HEAD"))
            .unwrap()
            .trim(),
        commit.to_string()
    );
}
