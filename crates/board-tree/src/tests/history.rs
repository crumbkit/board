//! A path's history: the commits that changed it, as `board log` walks
//! them (docs/architecture/records/history.md).

use super::*;
use crate::history::{Decision, Follow, decide};
use crate::{GitError, History, MAX_WALKED, Walk};

/// Every commit of these tests has its own committer time, from this one
/// on, so the walk's order is fixed.
const T: i64 = 1_700_000_000;

/// The history of `path` from HEAD, with `limit` and the full bound.
fn history(scenario: &Scenario, path: &str, limit: Option<usize>) -> History {
    let repository = scenario.open();
    let head = repository.head().unwrap().unwrap();
    let walk = Walk {
        limit,
        bound: MAX_WALKED,
    };
    repository.history(head, path, walk).unwrap()
}

/// The commits a history lists, newest first.
fn commits(history: &History) -> Vec<ObjectId> {
    history.changes.iter().map(|change| change.commit).collect()
}

#[test]
fn a_files_history_lists_the_commits_that_changed_it_newest_first_and_none_that_changed_only_others()
 {
    let scenario = Scenario::new();
    let first = scenario.commit_at(&[("a.json", "1")], &[], T, "add a");
    let other = scenario.commit_at(
        &[("a.json", "1"), ("b.json", "1")],
        &[first],
        T + 1,
        "add b",
    );
    let second = scenario.commit_at(
        &[("a.json", "2"), ("b.json", "1")],
        &[other],
        T + 2,
        "edit a",
    );
    let found = history(&scenario, "a.json", None);
    assert_eq!(commits(&found), [second, first]);
    let change = &found.changes[0];
    assert_eq!(
        (change.time, change.subject.as_str(), change.blob),
        (T + 2, "edit a", Some(scenario.blob("2")))
    );
    assert_eq!(found.walked, 3);
    assert!(!found.truncated() && found.complete());
    assert_eq!(commits(&history(&scenario, "b.json", None)), [other]);
}

#[test]
fn a_commit_that_removes_the_file_is_listed_with_no_blob() {
    let scenario = Scenario::new();
    let added = scenario.commit_at(&[("a.json", "1")], &[], T, "add");
    let removed = scenario.commit_at(&[("b.json", "1")], &[added], T + 1, "remove");
    let found = history(&scenario, "a.json", None);
    assert_eq!(commits(&found), [removed, added]);
    assert_eq!(found.changes[0].blob, None);
    assert_eq!(found.changes[1].blob, Some(scenario.blob("1")));
}

/// A root, a side that changes `a.json`, and one that changes only
/// `c.json`, ready for a merge.
fn forked(scenario: &Scenario) -> (ObjectId, ObjectId, ObjectId) {
    let root = scenario.commit_at(&[("a.json", "1")], &[], T, "root");
    let changed = scenario.commit_at(&[("a.json", "2")], &[root], T + 1, "change a");
    let other = scenario.commit_at(&[("a.json", "1"), ("c.json", "y")], &[root], T + 2, "add c");
    (root, changed, other)
}

#[test]
fn a_merge_whose_file_equals_one_parents_is_not_listed_and_only_that_parent_is_walked() {
    let scenario = Scenario::new();
    let (root, changed, other) = forked(&scenario);
    let files = [("a.json", "2"), ("c.json", "y")];
    scenario.commit_at(&files, &[other, changed], T + 3, "merge");
    let found = history(&scenario, "a.json", None);
    assert_eq!(commits(&found), [changed, root]);
    assert_eq!(found.walked, 3, "the merge, the changed side and the root");
}

#[test]
fn a_merge_whose_file_differs_from_every_parent_is_listed_and_every_parent_is_walked() {
    let scenario = Scenario::new();
    let (root, changed, other) = forked(&scenario);
    let files = [("a.json", "3"), ("c.json", "y")];
    let merge = scenario.commit_at(&files, &[other, changed], T + 3, "merge");
    let found = history(&scenario, "a.json", None);
    assert_eq!(commits(&found), [merge, changed, root]);
    assert_eq!(found.walked, 4);
}

#[test]
fn a_child_with_an_older_time_than_its_parent_is_still_listed_first() {
    let scenario = Scenario::new();
    let parent = scenario.commit_at(&[("a.json", "1")], &[], T + 10, "parent");
    let child = scenario.commit_at(&[("a.json", "2")], &[parent], T, "child");
    assert_eq!(
        commits(&history(&scenario, "a.json", None)),
        [child, parent]
    );
}

/// One history with a root, a merge that equals its first parent, a
/// removal, a merge that differs from both parents, and a side branch.
fn fixed(scenario: &Scenario) {
    let root = scenario.commit_at(&[("f.json", "1")], &[], T + 100, "a");
    let two = scenario.commit_at(&[("f.json", "2")], &[root], T + 200, "b");
    let side = scenario.commit_at(&[("f.json", "1"), ("g.json", "c")], &[root], T + 300, "c");
    let merge = scenario.commit_at(
        &[("f.json", "2"), ("g.json", "c")],
        &[two, side],
        T + 400,
        "d",
    );
    let removed = scenario.commit_at(&[("g.json", "c")], &[merge], T + 500, "e");
    let three = scenario.commit_at(
        &[("f.json", "3"), ("g.json", "c")],
        &[removed],
        T + 600,
        "f",
    );
    let four = scenario.commit_at(
        &[("f.json", "4"), ("g.json", "c")],
        &[removed],
        T + 650,
        "h",
    );
    scenario.commit_at(
        &[("f.json", "5"), ("g.json", "c")],
        &[three, four],
        T + 700,
        "g",
    );
}

/// `git log --format=%H -- f.json` in the repository `fixed` builds, run
/// once by hand with git 2.56.0: g, h, f, e, b and a.
const GIT_LOG: [&str; 6] = [
    "8e2d8ebd455788d0e3e6c86fdea8ba4ecc296112",
    "b39d90c9e6a48cdb4ef39c181b57031a252e5e1f",
    "2788d21d3a8c4006c59c43ed1135e8c3788d2dc0",
    "b2c2f9908d2b7295f8455d2cf79d32fd423173c9",
    "42ba85d97ebb7517bdb61dc49b5fedc137d2a440",
    "4bc59ccbd7361a907a325a4d8bfe4bc640c99a50",
];

#[test]
fn on_one_fixed_history_the_list_equals_what_git_log_gives() {
    let scenario = Scenario::new();
    fixed(&scenario);
    let listed: Vec<String> = commits(&history(&scenario, "f.json", None))
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(listed, GIT_LOG);
}

#[test]
fn a_directorys_history_lists_a_commit_that_changed_a_file_beneath_it() {
    let scenario = Scenario::new();
    let first = scenario.commit_at(&[("tasks/a.json", "1")], &[], T, "add");
    let outside = scenario.commit_at(
        &[("tasks/a.json", "1"), ("b.json", "1")],
        &[first],
        T + 1,
        "b",
    );
    let beneath = scenario.commit_at(
        &[("tasks/a.json", "2"), ("b.json", "1")],
        &[outside],
        T + 2,
        "a",
    );
    let found = history(&scenario, "tasks", None);
    assert_eq!(commits(&found), [beneath, first]);
    assert_eq!(found.changes[0].blob, None, "a directory has no blob");
    let root = history(&scenario, "", None);
    assert_eq!(commits(&root), [beneath, outside, first]);
}

/// A file changed only at HEAD, at the end of a history of five commits.
fn changed_at_head(scenario: &Scenario) -> ObjectId {
    let mut parent = scenario.commit_at(&[("b.json", "0")], &[], T, "0");
    for at in 1..4 {
        parent = scenario.commit_at(&[("b.json", &at.to_string())], &[parent], T + at, "b");
    }
    scenario.commit_at(&[("a.json", "1"), ("b.json", "3")], &[parent], T + 4, "a")
}

#[test]
fn a_limit_of_one_on_a_file_changed_at_the_start_reads_one_commit_and_is_truncated() {
    let scenario = Scenario::new();
    let head = changed_at_head(&scenario);
    let found = history(&scenario, "a.json", Some(1));
    assert_eq!(commits(&found), [head]);
    assert_eq!(found.walked, 1);
    assert!(found.truncated() && found.complete());
}

#[test]
fn with_no_limit_the_walk_reads_every_commit_gives_the_same_row_and_is_not_truncated() {
    let scenario = Scenario::new();
    let head = changed_at_head(&scenario);
    let found = history(&scenario, "a.json", None);
    assert_eq!(commits(&found), [head]);
    assert_eq!(found.walked, 5);
    assert!(!found.truncated() && found.complete());
}

#[test]
fn a_walk_bound_below_the_historys_length_returns_the_rows_found_truncated_and_not_complete() {
    let scenario = Scenario::new();
    changed_at_head(&scenario);
    let repository = scenario.open();
    let head = repository.head().unwrap().unwrap();
    let walk = |bound| Walk { limit: None, bound };
    let bounded = repository.history(head, "b.json", walk(3)).unwrap();
    assert_eq!((bounded.changes.len(), bounded.walked), (2, 3));
    assert!(bounded.truncated() && !bounded.complete());
    let whole = repository.history(head, "b.json", walk(6)).unwrap();
    assert_eq!((whole.changes.len(), whole.walked), (4, 5));
    assert!(!whole.truncated() && whole.complete());
}

#[test]
fn a_shallow_boundary_commit_is_walked_as_a_root_and_the_result_is_truncated_and_not_complete() {
    let shallow = Scenario::new();
    let full = Scenario::new();
    for (scenario, cut) in [(&shallow, true), (&full, false)] {
        let first = scenario.commit_at(&[("a.json", "1")], &[], T, "1");
        let boundary = scenario.commit_at(&[("a.json", "2")], &[first], T + 1, "2");
        scenario.commit_at(&[("a.json", "3")], &[boundary], T + 2, "3");
        if cut {
            scenario.write(".git/shallow", Item::File(&format!("{boundary}\n")));
        }
    }
    let cut = history(&shallow, "a.json", None);
    assert_eq!((cut.changes.len(), cut.walked), (2, 2));
    assert!(cut.truncated() && !cut.complete());
    let whole = history(&full, "a.json", None);
    assert_eq!((whole.changes.len(), whole.walked), (3, 3));
    assert!(!whole.truncated() && whole.complete());
}

proptest! {
    /// A commit is listed exactly when its entry differs from every
    /// parent's, and exactly one parent is walked when some parent's entry
    /// is equal: the first such parent.
    #[test]
    fn a_commit_is_listed_when_it_differs_from_every_parent_and_follows_the_first_equal_one(
        entry in proptest::option::of(0u8..3),
        parents in proptest::collection::vec(proptest::option::of(0u8..3), 0..4),
    ) {
        let Decision { listed, follow } = decide(entry.as_ref(), &parents);
        let differs = |at: usize| parents[at] != entry;
        let every_differs = (0..parents.len()).all(differs);
        let expected_listed = if parents.is_empty() { entry.is_some() } else { every_differs };
        prop_assert_eq!(listed, expected_listed);
        match follow {
            Follow::All => prop_assert!(every_differs),
            Follow::One(at) => {
                prop_assert!(at < parents.len() && !differs(at));
                prop_assert!((0..at).all(differs), "an earlier parent is equal");
            }
        }
    }
}

#[test]
fn commit_refuses_an_absent_id_and_a_blobs_and_admits_a_commits() {
    let scenario = Scenario::new();
    let commit = scenario.commit(&[("a.json", "1")]);
    let repository = scenario.open();
    let blob = scenario.blob("1");
    for refused in [ObjectId::Sha1([7; 20]), blob, ObjectId::Sha256([7; 32])] {
        let error = repository.commit(refused).unwrap_err();
        assert!(matches!(error, GitError::NoCommit { id, .. } if id == refused));
        assert_eq!(error.code(), Code::CommitNotFound);
    }
    assert_eq!(repository.commit(commit).unwrap(), commit);
}
