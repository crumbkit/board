//! A put whose HEAD moves between staging its commit and moving HEAD
//! (docs/architecture.md, Writes). The board's `between` seam runs on the
//! put's thread after staging, while the put holds `index.lock`, before each
//! compare-and-swap. It models a program that moves HEAD without taking
//! `index.lock`, such as `git update-ref`; a real `git commit` cannot
//! interleave there. The last test models one that holds `index.lock`.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use board_model::{Code, ObjectId};
use board_scenario::{Item, Scenario};

use super::{committing, path};
use crate::Board;
use crate::board::Between;
use crate::put::{Expect, PutError, PutReport, Removing, Unmatched};
use crate::tests::{open, task};

type Shared = Arc<Mutex<Scenario>>;

/// The committing board with a first commit for HEAD to move from.
fn seeded() -> Shared {
    let scenario = committing();
    scenario.commit(&[("seed.txt", "seed")]);
    Arc::new(Mutex::new(scenario))
}

/// Commits what HEAD holds, plus `extra` (a path and its text), as another
/// program would.
fn commit_beside(scenario: &Scenario, extra: &str, text: &str) {
    let mut files = scenario.head().unwrap().files;
    files.insert(extra.to_owned(), text.as_bytes().to_vec());
    let files: Vec<(&str, String)> = files
        .iter()
        .map(|(path, bytes)| (path.as_str(), String::from_utf8_lossy(bytes).into_owned()))
        .collect();
    let files: Vec<(&str, &str)> = files.iter().map(|(p, t)| (*p, t.as_str())).collect();
    scenario.commit(&files);
}

/// A board whose `between` runs `hook` with the scenario locked.
fn board_moving(shared: &Shared, hook: impl Fn(usize, &Scenario) + Send + 'static) -> Board {
    let mut board = open(&shared.lock().unwrap());
    let shared = Arc::clone(shared);
    board.between = Between::new(move |attempt| hook(attempt, &shared.lock().unwrap()));
    board
}

fn put_task(board: &Board) -> Result<PutReport, PutError> {
    board.put(&path("/tasks/a"), Expect::Any, task("").as_bytes())
}

#[test]
fn an_unrelated_commit_under_a_put_is_admitted_on_the_new_head() {
    let shared = seeded();
    let board = board_moving(&shared, |attempt, scenario| {
        if attempt == 0 {
            commit_beside(scenario, "other.txt", "{}");
        }
    });
    let commit = put_task(&board).unwrap().commit.unwrap();
    let head = shared.lock().unwrap().head().unwrap();
    assert_eq!(head.id, commit);
    assert!(
        head.files.contains_key("other.txt"),
        "the other commit is kept"
    );
    assert!(head.files.contains_key("tasks/a.json"));
    assert_eq!(head.parents.len(), 1);
}

#[test]
fn a_commit_to_the_puts_path_under_it_refuses_head_moved() {
    let shared = seeded();
    let board = board_moving(&shared, |attempt, scenario| {
        if attempt == 0 {
            commit_beside(scenario, "tasks/a.json", "{}");
        }
    });
    let error = put_task(&board).unwrap_err();
    assert!(matches!(error, PutError::HeadMoved { .. }), "{error}");
    assert_eq!(error.code(), Code::PutHeadMoved);
}

#[test]
fn a_head_that_moves_on_every_attempt_refuses_after_three_retries_at_the_last_commit() {
    let shared = seeded();
    let attempts = Arc::new(Mutex::new(0));
    let counted = Arc::clone(&attempts);
    let board = board_moving(&shared, move |attempt, scenario| {
        *counted.lock().unwrap() = attempt + 1;
        commit_beside(scenario, &format!("other{attempt}.txt"), "{}");
    });
    let error = put_task(&board).unwrap_err();
    assert!(matches!(error, PutError::HeadMoved { .. }), "{error}");
    assert_eq!(
        *attempts.lock().unwrap(),
        4,
        "the first attempt and three retries"
    );
    let head = shared.lock().unwrap().head().unwrap();
    assert!(
        head.files.contains_key("other3.txt"),
        "HEAD is the last mover's"
    );
    assert!(
        !head.files.contains_key("tasks/a.json"),
        "the put committed nothing"
    );
}

#[test]
fn a_removal_is_staged_again_on_the_new_head() {
    let shared = seeded();
    let first = put_task(&open(&shared.lock().unwrap())).unwrap();
    assert!(first.commit.is_some());
    let board = board_moving(&shared, |attempt, scenario| {
        if attempt == 0 {
            commit_beside(scenario, "other.txt", "{}");
        }
    });
    let report = board
        .remove_waiting(
            &path("/tasks/a"),
            Removing::File(Expect::Any),
            Unmatched::Refuse,
            None,
            Duration::from_secs(5),
        )
        .unwrap();
    let head = shared.lock().unwrap().head().unwrap();
    assert_eq!(report.commit, Some(head.id));
    assert!(head.files.contains_key("other.txt"));
    assert!(!head.files.contains_key("tasks/a.json"));
}

/// The commit cannot be staged again once the files are written, for here a
/// merge began meanwhile. The put refuses `head_moved`, the files stay
/// written and uncommitted, and the next sync adopts them.
#[test]
fn a_put_that_cannot_stage_again_refuses_head_moved_and_leaves_its_files() {
    let shared = seeded();
    let board = board_moving(&shared, |attempt, scenario| {
        if attempt == 0 {
            commit_beside(scenario, "other.txt", "{}");
            let head = scenario.head().unwrap().id;
            scenario.write(".git/MERGE_HEAD", Item::File(&format!("{head}\n")));
        }
    });
    let error = put_task(&board).unwrap_err();
    assert!(matches!(error, PutError::HeadMoved { .. }), "{error}");
    let scenario = shared.lock().unwrap();
    assert!(scenario.workdir().join("tasks/a.json").exists());
    assert!(!scenario.head().unwrap().files.contains_key("tasks/a.json"));
}

/// A put compares `--expect` before it waits for `index.lock`, so a program
/// that holds the lock, changes a path the put writes and moves HEAD during
/// that wait must not be overwritten.
fn put_while_index_locked(change: impl FnOnce(&Scenario)) -> Result<ObjectId, PutError> {
    let shared = seeded();
    let scenario = shared.lock().unwrap();
    scenario.write(".git/index.lock", Item::File(""));
    let root = scenario.workdir().to_path_buf();
    let board_lock = root.join(".git/board.lock");
    let result = std::thread::scope(|scope| {
        let put = scope.spawn(move || {
            let board = Board::open(&root, None).unwrap();
            put_task(&board).map(|report| report.commit.unwrap())
        });
        // The put holds board.lock from before it compares --expect.
        while !board_lock.exists() {
            std::thread::sleep(Duration::from_millis(10));
        }
        std::thread::sleep(Duration::from_millis(300));
        change(&scenario);
        scenario.write(".git/index.lock", Item::Deleted);
        put.join().unwrap()
    });
    drop(scenario);
    result
}

#[test]
fn a_commit_to_the_puts_path_while_index_lock_is_held_refuses_head_moved() {
    let error = put_while_index_locked(|scenario| commit_beside(scenario, "tasks/a.json", "{}"))
        .unwrap_err();
    assert!(matches!(error, PutError::HeadMoved { .. }), "{error}");
}

#[test]
fn an_unrelated_commit_while_index_lock_is_held_is_admitted() {
    put_while_index_locked(|scenario| commit_beside(scenario, "other.txt", "{}")).unwrap();
}
