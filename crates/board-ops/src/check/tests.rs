//! Checking through the index: what it says against HEAD, how it shares the
//! index with other processes, and that a warm check answers as a cold one.

use std::time::{Duration, Instant};

use board_index::{Fallback, Index};
use board_model::{Change, Code, Kind, Source};
use board_scenario::{Item, Scenario};
use board_tree::Status;

use crate::check::Tally;
use crate::tests::{PROJECT_SCHEMA, TASK_SCHEMA, board, open, stand_in, task};
use crate::{CheckReport, Checked, Detail, Refreshed};

mod kept;

fn changes(scenario: &Scenario) -> Vec<(String, Change)> {
    open(scenario)
        .check(None, None)
        .unwrap()
        .entries
        .into_iter()
        .map(|entry| (entry.path, entry.change))
        .collect()
}

#[test]
fn an_entry_that_differs_from_head_is_uncommitted() {
    let scenario = Scenario::new();
    let config = r#"{"$type": "board/config/v1"}"#;
    let commit = scenario.commit(&[
        (".board/board.json", config),
        ("same.md", "1"),
        ("edited.md", "1"),
    ]);
    scenario
        .track(commit)
        .write(".board/board.json", Item::File(config))
        .write("same.md", Item::File("1"))
        .write("edited.md", Item::File("2"))
        .write("new.md", Item::File("1"));
    let expected = [
        (".board/board.json", Change::Committed),
        ("edited.md", Change::Uncommitted),
        ("new.md", Change::Uncommitted),
        ("same.md", Change::Committed),
    ];
    let expected: Vec<(String, Change)> = expected
        .into_iter()
        .map(|(path, change)| (path.to_owned(), change))
        .collect();
    assert_eq!(changes(&scenario), expected);
    scenario.write(".git/HEAD", Item::File(&format!("{commit}\n")));
    assert_eq!(changes(&scenario), expected, "a detached HEAD is the same");
}

#[test]
fn before_the_first_commit_every_entry_is_uncommitted() {
    let scenario = board();
    assert!(
        changes(&scenario)
            .iter()
            .all(|(_, change)| *change == Change::Uncommitted)
    );
}

/// A board whose every file is committed and in the working tree as
/// committed, with one task referring to the project.
fn committed_board() -> Scenario {
    let scenario = Scenario::new();
    let task = task(r#", "project": "/projects/p""#);
    let files = [
        (".board/board.json", r#"{"$type": "board/config/v1"}"#),
        (".board/types/lr/task/v1.schema.json", TASK_SCHEMA),
        (".board/types/lr/project/v1.schema.json", PROJECT_SCHEMA),
        ("projects/p.json", r#"{"$type": "lr/project/v1"}"#),
        ("tasks/a.json", task.as_str()),
    ];
    scenario.commit(&files);
    for (path, text) in files {
        scenario.write(path, Item::File(text));
    }
    scenario
}

#[test]
fn a_check_with_nothing_changed_does_no_work_and_reads_while_another_process_writes() {
    let scenario = committed_board();
    let cache = tempfile::tempdir().unwrap();
    let board = open(&scenario);
    let cold = board.check(None, Some(cache.path())).unwrap();
    assert!(cold.work.wrote, "{:?}", cold.work);
    let other = Index::open(Some(cache.path()), board.repository().path()).unwrap();
    let writing = other.write().unwrap();
    let warm = board.check(None, Some(cache.path())).unwrap();
    assert_eq!(warm.memory, None, "it used the file without waiting");
    // The board holds no pointer, and the project the task refers to is
    // not read, because its facts say it holds none.
    assert_eq!(warm.work, Refreshed::default(), "nothing is read again");
    assert_eq!(warm.entries, cold.entries);
    drop(writing);
}

#[test]
fn a_check_after_a_change_writes_what_changed() {
    let scenario = committed_board();
    let cache = tempfile::tempdir().unwrap();
    let board = open(&scenario);
    board.check(None, Some(cache.path())).unwrap();
    scenario.write("tasks/a.json", Item::File(&task("")));
    let warm = board.check(None, Some(cache.path())).unwrap();
    assert_eq!(warm.memory, None);
    assert_eq!(
        warm.work,
        Refreshed {
            hashed: 1,
            parsed: 1,
            compiled: 1,
            validated: 1,
            wrote: true,
            ..Refreshed::default()
        }
    );
}

/// Checks `scenario` after a change, with or without another process
/// holding the index's write lock throughout.
fn check_changed_while(held: bool) -> CheckReport {
    let scenario = committed_board();
    let cache = tempfile::tempdir().unwrap();
    let board = open(&scenario);
    board.check(None, Some(cache.path())).unwrap();
    scenario.write("tasks/a.json", Item::File(&task("")));
    let other = Index::open(Some(cache.path()), board.repository().path()).unwrap();
    let writing = held.then(|| other.write().unwrap());
    let report = board.check(None, Some(cache.path())).unwrap();
    drop(writing);
    assert_eq!(
        report.entries,
        board.check(None, None).unwrap().entries,
        "as from a cold index"
    );
    report
}

#[test]
fn a_check_that_must_write_while_another_process_holds_the_lock_works_in_memory() {
    let started = Instant::now();
    let report = check_changed_while(true);
    assert_eq!(report.memory, Some(Fallback::Busy));
    // The documented wait for the lock is 5 seconds.
    assert!(started.elapsed() >= Duration::from_secs(5));
}

#[test]
fn a_check_that_must_write_after_another_process_let_go_writes_the_file() {
    let report = check_changed_while(false);
    assert_eq!(report.memory, None);
    assert!(report.work.wrote);
}

#[test]
fn a_file_whose_bytes_are_not_the_blob_the_walk_saw_is_unreadable_and_its_twin_is_ok() {
    let scenario = board();
    scenario
        .write("tasks/a.json", Item::File(&task("")))
        .write("tasks/b.json", Item::File(&task("")));
    let cache = tempfile::tempdir().unwrap();
    let board = open(&scenario);
    board.check(None, Some(cache.path())).unwrap();
    let held = scenario.blob("what the file held");
    stand_in(&board, cache.path(), "tasks/a.json", held);
    let report = board.check(None, Some(cache.path())).unwrap();
    let entry = |path: &str| {
        let found = report.entries.iter().find(|entry| entry.path == path);
        found.unwrap().clone()
    };
    let changed = entry("tasks/a.json");
    assert_eq!(changed.status, Status::Degraded(Code::RepoUnreadable));
    let detail = changed.detail.unwrap();
    assert_eq!(
        detail.reason,
        "changed or could not be read; run the command again"
    );
    assert_eq!(detail.failures, 1);
    assert_eq!(changed.schema, None);
    let twin = entry("tasks/b.json");
    assert_eq!(twin.status, Status::Ok);
    assert_eq!(twin.detail, None);
}

#[test]
fn a_record_whose_schema_changed_during_the_check_is_unreadable_and_one_under_an_unchanged_schema_is_ok()
 {
    let scenario = board();
    scenario.write("tasks/a.json", Item::File(&task("")));
    let cache = tempfile::tempdir().unwrap();
    let board = open(&scenario);
    board.check(None, Some(cache.path())).unwrap();
    let schema = ".board/types/lr/task/v1.schema.json";
    stand_in(
        &board,
        cache.path(),
        schema,
        scenario.blob("what the schema held"),
    );
    let report = board.check(None, Some(cache.path())).unwrap();
    let entry = |path: &str| {
        let found = report.entries.iter().find(|entry| entry.path == path);
        found.unwrap().clone()
    };
    let task = entry("tasks/a.json");
    assert_eq!(task.status, Status::Degraded(Code::RepoUnreadable));
    assert_eq!(task.schema.as_deref(), Some(schema));
    assert_eq!(
        task.detail.unwrap().reason,
        format!("cannot be checked: its schema at {schema} could not be read")
    );
    assert_eq!(entry("projects/p.json").status, Status::Ok);
}

#[test]
fn a_warm_check_hashes_nothing_even_resolving_references_into_a_large_directory() {
    let scenario = board();
    for at in 0..200 {
        scenario.write(
            &format!("projects/q{at}.json"),
            Item::File(r#"{"$type": "lr/project/v1"}"#),
        );
        scenario.write(
            &format!("tasks/t{at}.json"),
            Item::File(&task(&format!(r#", "project": "/projects/q{at}""#))),
        );
    }
    let cache = tempfile::tempdir().unwrap();
    let board = open(&scenario);
    let cold = board.check(None, Some(cache.path())).unwrap();
    assert!(!cold.degraded());
    assert_eq!(cold.work.hashed, 404, "every file, once");
    let warm = board.check(None, Some(cache.path())).unwrap();
    assert_eq!(warm.work.hashed, 0);
    scenario.write(
        "projects/q7.json",
        Item::File(r#"{"$type": "lr/project/v1", "x": 1}"#),
    );
    assert_eq!(
        board.check(None, Some(cache.path())).unwrap().work.hashed,
        1
    );
}

#[test]
fn a_tally_names_its_first_failing_reference_as_a_predicate_and_counts_every_one() {
    let dangling = (Code::RefDangling, "nothing is at /nowhere".to_owned());
    let wrong = (
        Code::RefWrongType,
        "/p is not a record of type lr/project/v1".to_owned(),
    );
    let mut tally = Tally::default();
    tally.add("/see", None);
    tally.add("/project", Some(&dangling));
    tally.add("/parent", Some(&wrong));
    assert_eq!(
        tally.first(),
        Some((
            Code::RefDangling,
            Detail {
                pointer: Some("/project".to_owned()),
                reason: r#"holds a reference at "/project": nothing is at /nowhere"#.to_owned(),
                failures: 2,
                ..Detail::default()
            }
        ))
    );
    let mut clean = Tally::default();
    clean.add("/see", None);
    clean.add("/project", None);
    assert_eq!(clean.first(), None);
}

#[test]
fn a_degraded_entry_hints_at_its_fix_where_its_file_is_and_an_ok_one_has_no_hint() {
    let degraded = Checked {
        source: Source::WorkingTree,
        path: "tasks/a.json".to_owned(),
        logical: Some("/tasks/a".parse().unwrap()),
        kind: Kind::Json,
        status: Status::Degraded(Code::JsonMergeConflict),
        detail: None,
        change: Change::Uncommitted,
        schema: None,
    };
    assert_eq!(
        degraded.hint().as_deref(),
        Some("resolve the git merge conflict in the file, then run `board check /tasks/a`")
    );
    let mounted = Checked {
        detail: Some(Detail {
            at: Some("/vendor/a".parse().unwrap()),
            mount: Some("/vendor".parse().unwrap()),
            ..Detail::default()
        }),
        status: Status::Degraded(Code::JsonParse),
        ..degraded.clone()
    };
    let hint = mounted.hint().unwrap();
    assert!(
        hint.starts_with("/vendor/a is in the repository of the board mounted at /vendor"),
        "{hint}"
    );
    let invalid = Checked {
        status: Status::Degraded(Code::RecordInvalid),
        detail: Some(Detail {
            pointer: Some("/due".to_owned()),
            ..Detail::default()
        }),
        ..degraded.clone()
    };
    assert!(invalid.hint().unwrap().contains("`/due`"));
    let untyped = Checked {
        status: Status::Degraded(Code::RecordUnknownType),
        schema: Some(".board/types/task.json".to_owned()),
        ..degraded.clone()
    };
    assert!(untyped.hint().unwrap().contains("`.board/types/task.json`"));
    let ok = Checked {
        status: Status::Ok,
        ..degraded
    };
    assert_eq!(ok.hint(), None);
}
