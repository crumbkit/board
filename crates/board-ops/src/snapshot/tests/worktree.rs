//! The working tree's decision, which the index keeps under a key of all it
//! depends on. A warm query or check of an unchanged board decides no entry,
//! and after each change decides the working tree again and answers as a
//! cold one.

use std::path::Path;

use board_index::Index;
use board_scenario::{Item, Scenario};

use super::decisions::query_as;
use super::{Need, prune};
use crate::tests::{Library, board, open, task};

/// A change to make between two queries, and what it is.
type Change<'s> = (&'static str, Box<dyn Fn() + 's>);

/// How many entries a warm query of `scenario` through the index in
/// `cache` decided, after it answered as a cold one.
fn decided(scenario: &Scenario, cache: &Path) -> usize {
    query_as(scenario, cache, "test").decided
}

/// A board that mounts the library, with tasks that refer into it and into
/// the board, an ignore rule and a file it ignores.
fn mounting() -> (Scenario, Library) {
    let scenario = board();
    let vendor = Library::mounted(&scenario);
    scenario
        .write(".gitignore", Item::File("ignored.json\n"))
        .write("ignored.json", Item::File(&task("")))
        .write(
            "tasks/a.json",
            Item::File(&task(r#", "project": "/projects/p""#)),
        )
        .write("tasks/b.json", Item::File(&task(r#", "see": "/tasks/a""#)));
    (scenario, vendor)
}

#[test]
fn a_warm_query_of_an_unchanged_board_decides_no_entry_and_a_cold_one_decides_each() {
    let (scenario, _vendor) = mounting();
    let cache = tempfile::tempdir().unwrap();
    assert!(decided(&scenario, cache.path()) > 0, "a cold index decides");
    assert_eq!(decided(&scenario, cache.path()), 0);
}

#[test]
fn a_build_that_names_no_build_decides_every_entry_each_time_and_its_named_twin_keeps_them() {
    for (build, keeps) in [("", false), ("test", true)] {
        let (scenario, _vendor) = mounting();
        let cache = tempfile::tempdir().unwrap();
        query_as(&scenario, cache.path(), build);
        let warm = query_as(&scenario, cache.path(), build);
        assert_eq!(warm.decided == 0, keeps, "{build:?}");
    }
}

#[test]
fn after_each_change_a_warm_query_decides_again_and_answers_as_a_cold_one() {
    let (scenario, vendor) = mounting();
    let cache = tempfile::tempdir().unwrap();
    let pruned = || {
        let board = open(&scenario);
        prune(Index::open(Some(cache.path()), board.repository().path()).unwrap());
    };
    let changes: Vec<Change<'_>> = vec![
        (
            "an edit in place of the same size",
            Box::new(|| {
                scenario.write(
                    "tasks/a.json",
                    Item::File(&task(r#", "project": "/projects/q""#)),
                );
            }),
        ),
        (
            "a rename",
            Box::new(|| {
                let text = task(r#", "see": "/tasks/a""#);
                scenario
                    .write("tasks/c.json", Item::File(&text))
                    .write("tasks/b.json", Item::Deleted);
            }),
        ),
        (
            "a delete",
            Box::new(|| {
                scenario.write("tasks/c.json", Item::Deleted);
            }),
        ),
        (
            "a commit that moves HEAD",
            Box::new(|| {
                scenario.commit(&[("projects/p.json", r#"{"$type": "lr/project/v1"}"#)]);
            }),
        ),
        (
            "an ignore rule",
            Box::new(|| {
                scenario.write(".gitignore", Item::File("ignored.json\ntasks/a.json\n"));
            }),
        ),
        (
            "an ignored file added to git's index",
            Box::new(|| {
                let commit = scenario.commit(&[("ignored.json", &task(""))]);
                scenario.track(commit);
            }),
        ),
        (
            "a sibling whose name differs only in case",
            Box::new(|| {
                // A file system that folds case writes over projects/p.json.
                let project = r#"{"$type": "lr/project/v1", "n": 1}"#;
                scenario.write("Projects/P.json", Item::File(project));
            }),
        ),
        (
            "a schema change",
            Box::new(|| {
                let schema = r#"{"type": "object", "required": ["title", "due"]}"#;
                scenario.write(".board/types/lr/task/v1.schema.json", Item::File(schema));
            }),
        ),
        ("a mount moved", Box::new(|| vendor.moved(&scenario))),
        ("a prune between runs", Box::new(pruned)),
        (
            "a file dated after the clock",
            Box::new(|| {
                scenario.write("tasks/d.json", Item::Ahead(&task("")));
            }),
        ),
    ];
    assert!(decided(&scenario, cache.path()) > 0);
    assert_eq!(decided(&scenario, cache.path()), 0, "nothing changed");
    for (change, make) in changes {
        make();
        assert!(decided(&scenario, cache.path()) > 0, "{change}");
        assert_eq!(
            decided(&scenario, cache.path()),
            0,
            "{change}, then nothing"
        );
    }
}

#[test]
fn a_decision_whose_write_rolled_back_is_not_kept_and_the_next_query_decides_again() {
    let (scenario, _vendor) = mounting();
    let cache = tempfile::tempdir().unwrap();
    let board = open(&scenario);
    let index = Index::open(Some(cache.path()), board.repository().path()).unwrap();
    let writer = index.write().unwrap();
    board.decide(&writer, None, Need::View).unwrap();
    writer.rollback().unwrap();
    assert!(decided(&scenario, cache.path()) > 0);
    assert_eq!(decided(&scenario, cache.path()), 0);
}

#[test]
fn a_warm_check_and_a_warm_query_of_an_unchanged_board_read_no_validation() {
    let (scenario, _vendor) = mounting();
    let cache = tempfile::tempdir().unwrap();
    decided(&scenario, cache.path());
    let board = open(&scenario);
    for need in [Need::View, Need::Check] {
        let index = Index::open(Some(cache.path()), board.repository().path()).unwrap();
        let (read, _, _) = crate::index::transact(index, |writer| {
            let (_, entries) = board.decide(writer, None, need)?;
            Ok((writer.validations_read(), entries))
        })
        .unwrap();
        assert_eq!(read, 0, "{need:?}");
    }
}
