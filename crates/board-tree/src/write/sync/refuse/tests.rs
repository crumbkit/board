use board_scenario::Scenario;

use super::*;
use crate::git::{Repository, model_id};

/// A commit id the store does not hold.
fn pruned() -> ObjectId {
    model_id(&gix::ObjectId::from_hex(b"1234567890123456789012345678901234567890").unwrap())
}

#[test]
fn a_previous_commit_missing_from_the_store_refuses_and_says_the_ref_stayed() {
    let remote = Scenario::new();
    remote.commit(&[("a.json", "{}")]);
    let clone = Scenario::clone_of(&remote);
    let repository = Repository::open(clone.workdir()).unwrap();
    let upstream = repository.upstream().unwrap();
    let fetched = repository.tracking(&upstream).unwrap();
    let before = Before {
        previous: Some(pruned()),
        restore: Some(pruned()),
    };
    let error = repository
        .refuse_unsafe(&upstream, fetched, before)
        .unwrap_err();
    assert!(
        matches!(
            error,
            SyncError::Rewound {
                unrestored: true,
                ..
            }
        ),
        "{error}"
    );
    assert!(
        error.to_string().contains("could not be put back"),
        "{error}"
    );
    assert_eq!(repository.tracking(&upstream).unwrap(), fetched);
}

#[test]
fn a_ref_that_no_longer_matches_the_fetched_commit_is_left_and_reported() {
    let remote = Scenario::new();
    let first = remote.commit(&[("a.json", "{}")]);
    let clone = Scenario::clone_of(&remote);
    let second = remote.commit(&[("a.json", "{}"), ("b.json", "[]")]);
    let repository = Repository::open(clone.workdir()).unwrap();
    let upstream = repository.upstream().unwrap();
    clone.write(".git/MERGE_HEAD", board_scenario::Item::File("x\n"));
    let before = Before {
        previous: None,
        restore: Some(first),
    };
    // The clone's tracking ref is at `first`; a refusal that names `second`
    // as fetched must not move a ref that no longer matches it.
    let error = repository
        .refuse_unsafe(&upstream, second, before)
        .unwrap_err();
    assert!(
        matches!(
            error,
            SyncError::Merging {
                unrestored: true,
                ..
            }
        ),
        "{error}"
    );
}

#[test]
fn an_unreadable_tracking_ref_is_an_error_and_a_missing_one_is_a_first_sync() {
    let remote = Scenario::new();
    remote.commit(&[("a.json", "{}")]);
    let clone = Scenario::clone_of(&remote);
    let repository = Repository::open(clone.workdir()).unwrap();
    let upstream = repository.upstream().unwrap();
    clone.write(
        ".git/refs/remotes/origin/main",
        board_scenario::Item::File("not a commit id\n"),
    );
    assert!(repository.tracking_before(&upstream).is_err());
    clone.write(
        ".git/refs/remotes/origin/main",
        board_scenario::Item::Deleted,
    );
    assert_eq!(repository.tracking_before(&upstream).unwrap(), None);
}
