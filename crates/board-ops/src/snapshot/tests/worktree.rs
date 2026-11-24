//! The working tree's decision, which the index keeps in a slot with the
//! key of all it depends on. A warm query or check of an unchanged board
//! decides no entry, and after each change decides again only the entries
//! the change could alter, and answers as a cold one.

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

/// A note that links to three directories, then those directories made
/// empty, removed and hidden by a rule in `.git/info/exclude`, which no
/// entry holds: a resolution reaches a directory that holds no entry. Each
/// step leaves a set of directories no earlier step left, as a set seen
/// before keeps its decision.
fn directories(scenario: &Scenario) -> Vec<Change<'_>> {
    let made = |name: &str| {
        let file = format!("topics/{name}/x.json");
        scenario
            .write(&file, Item::File(&task("")))
            .write(&file, Item::Deleted);
    };
    vec![
        (
            "a note that links to directories not made yet",
            Box::new(|| {
                let note = "[a](/topics/a) [b](/topics/b) [c](/topics/c)\n";
                scenario.write("notes/n.md", Item::File(note));
            }),
        ),
        ("an empty directory made", Box::new(move || made("a"))),
        ("a second empty directory made", Box::new(move || made("b"))),
        (
            "an empty directory removed",
            Box::new(|| {
                scenario.write("topics/a", Item::Deleted);
            }),
        ),
        (
            "an empty directory made and another hidden by an exclude rule outside the tree",
            Box::new(move || {
                made("c");
                scenario.write(".git/info/exclude", Item::File("topics/b/\n"));
            }),
        ),
    ]
}

#[test]
fn after_each_change_a_warm_query_decides_again_and_answers_as_a_cold_one() {
    let (scenario, vendor) = mounting();
    // Enough other entries that most changes decide only what they reach.
    fill(&scenario, 80);
    let cache = tempfile::tempdir().unwrap();
    let pruned = || {
        let board = open(&scenario);
        prune(Index::open(Some(cache.path()), board.repository().path()).unwrap());
    };
    let mut changes: Vec<Change<'_>> = vec![
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
    changes.extend(directories(&scenario));
    changes.extend(more(&scenario));
    assert!(decided(&scenario, cache.path()) > 0);
    assert_eq!(decided(&scenario, cache.path()), 0, "nothing changed");
    for (change, make) in changes {
        make();
        let count = decided(&scenario, cache.path());
        // A commit changes no entry, so no entry is decided again; only
        // each row's `change` is set again. No entry refers to the file
        // deleted.
        let none = ["a commit that moves HEAD", "a delete"].contains(&change);
        assert_eq!(count == 0, none, "{change}: {count}");
        assert_eq!(
            decided(&scenario, cache.path()),
            0,
            "{change}, then nothing"
        );
    }
}

/// Changes to repo records, a symlink and links through it, each answered
/// as a cold run answers it.
fn more(scenario: &Scenario) -> Vec<Change<'_>> {
    let repo = |remote: &str| format!(r#"{{"$type": "board/repo/v1", "remote": "{remote}"}}"#);
    vec![
        (
            "a repo record",
            Box::new(move || {
                scenario.write("repos/a.json", Item::File(&repo("example-org/a")));
            }),
        ),
        (
            "a second repo record that names the first's remote",
            Box::new(move || {
                scenario.write("repos/b.json", Item::File(&repo("example-org/a")));
            }),
        ),
        (
            "a symlink and a note that links through it",
            Box::new(|| {
                scenario
                    .write("linked/x.json", Item::Symlink("../projects/p.json"))
                    .write("notes/l.md", Item::File("[x](/linked/x)\n"));
            }),
        ),
        (
            "the symlink retargeted",
            Box::new(|| {
                scenario
                    .write("linked/x.json", Item::Deleted)
                    .write("linked/x.json", Item::Symlink("../tasks/a.json"));
            }),
        ),
    ]
}

/// Writes `count` tasks in `topics/` that refer to nothing.
fn fill(scenario: &Scenario, count: usize) {
    for at in 0..count {
        let fields = format!(r#", "note": "{at}""#);
        scenario.write(&format!("topics/t{at}.json"), Item::File(&task(&fields)));
    }
}

/// `mounting`'s board with `count` more tasks in `topics/`, which refer to
/// nothing, and a task whose reference to `/projects/z` dangles.
fn crowded(count: usize) -> (Scenario, Library) {
    let (scenario, vendor) = mounting();
    fill(&scenario, count);
    let dangling = task(r#", "see": "/projects/z""#);
    scenario.write("tasks/z.json", Item::File(&dangling));
    (scenario, vendor)
}

#[test]
fn after_a_record_is_added_a_warm_query_decides_it_and_each_entry_whose_reference_reaches_it() {
    let (scenario, _vendor) = crowded(60);
    let cache = tempfile::tempdir().unwrap();
    decided(&scenario, cache.path());
    assert_eq!(decided(&scenario, cache.path()), 0);
    let project = r#"{"$type": "lr/project/v1"}"#;
    scenario.write("projects/q.json", Item::File(project));
    assert_eq!(
        decided(&scenario, cache.path()),
        1,
        "a record nothing refers to"
    );
    scenario.write("projects/z.json", Item::File(project));
    assert_eq!(
        decided(&scenario, cache.path()),
        2,
        "a record a task refers to"
    );
}

#[test]
fn a_commit_that_changes_no_entry_decides_none_and_sets_each_rows_change_as_a_cold_run_does() {
    let (scenario, _vendor) = crowded(60);
    let cache = tempfile::tempdir().unwrap();
    decided(&scenario, cache.path());
    let committed = task(r#", "project": "/projects/p""#);
    scenario.commit(&[("tasks/a.json", &committed)]);
    assert_eq!(decided(&scenario, cache.path()), 0);
    scenario.commit(&[("tasks/b.json", &task(r#", "see": "/tasks/a""#))]);
    assert_eq!(decided(&scenario, cache.path()), 0, "a second commit");
}

#[test]
fn more_than_one_in_twenty_entries_changed_decides_every_entry_and_fewer_decide_those_changed() {
    for (changed, every) in [(1, false), (10, true)] {
        let (scenario, _vendor) = crowded(60);
        let cache = tempfile::tempdir().unwrap();
        decided(&scenario, cache.path());
        for at in 0..changed {
            let fields = format!(r#", "note": "changed {at}""#);
            scenario.write(&format!("topics/t{at}.json"), Item::File(&task(&fields)));
        }
        let count = decided(&scenario, cache.path());
        let expected = if every { count > 60 } else { count == changed };
        assert!(expected, "{changed} changed decided {count}");
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

#[test]
fn a_warm_check_and_a_warm_query_of_an_unchanged_board_write_nothing_and_a_cold_one_writes() {
    let (scenario, _vendor) = mounting();
    let cache = tempfile::tempdir().unwrap();
    let board = open(&scenario);
    let wrote = |need| {
        let index = Index::open(Some(cache.path()), board.repository().path()).unwrap();
        let (wrote, _, _) = crate::index::transact(index, |writer| {
            let (_, entries) = board.decide(writer, None, need)?;
            Ok((writer.wrote()?, entries))
        })
        .unwrap();
        wrote
    };
    assert!(wrote(Need::View), "a cold query writes");
    for need in [Need::View, Need::Check] {
        assert!(!wrote(need), "{need:?}");
    }
}
