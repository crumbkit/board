//! A put whose HEAD moves between staging its commit and moving HEAD
//! (docs/architecture.md, Writes). The hook runs on the put's own thread,
//! after staging and before each compare-and-swap.

use std::rc::Rc;

use board_model::Code;
use board_scenario::Scenario;

use super::{committing, path};
use crate::put::{Expect, PutError};
use crate::tests::{open, task};

/// Commits what HEAD holds, plus `extra`, as another program would.
fn commit_beside(scenario: &Scenario, extra: &str) {
    let files = scenario.head().unwrap().files;
    let files: Vec<(&str, String)> = files
        .iter()
        .map(|(path, bytes)| (path.as_str(), String::from_utf8_lossy(bytes).into_owned()))
        .chain([(extra, "{}".to_owned())])
        .collect();
    let files: Vec<(&str, &str)> = files.iter().map(|(p, t)| (*p, t.as_str())).collect();
    scenario.commit(&files);
}

/// The committing board with a first commit for HEAD to move from.
fn seeded() -> Rc<Scenario> {
    let scenario = committing();
    scenario.commit(&[("seed.txt", "seed")]);
    Rc::new(scenario)
}

fn put_task(scenario: &Scenario) -> Result<crate::put::PutReport, PutError> {
    open(scenario).put(&path("/tasks/a"), Expect::Any, task("").as_bytes())
}

#[test]
fn an_unrelated_commit_under_a_put_is_admitted_on_the_new_head() {
    let scenario = seeded();
    let mover = Rc::clone(&scenario);
    let _hook = board_tree::before_head_moves(move |attempt| {
        if attempt == 0 {
            commit_beside(&mover, "other.txt");
        }
    });
    let moved_to = {
        let report = put_task(&scenario).unwrap();
        report.commit.unwrap()
    };
    let head = scenario.head().unwrap();
    assert_eq!(head.id, moved_to);
    assert!(
        head.files.contains_key("other.txt"),
        "the other commit is kept"
    );
    assert!(head.files.contains_key("tasks/a.json"));
    assert_eq!(head.parents.len(), 1);
}

#[test]
fn a_commit_to_the_puts_path_under_it_refuses_head_moved() {
    let scenario = seeded();
    let mover = Rc::clone(&scenario);
    let _hook = board_tree::before_head_moves(move |attempt| {
        if attempt == 0 {
            let mut files = mover.head().unwrap().files;
            files.insert("tasks/a.json".to_owned(), b"{}".to_vec());
            let files: Vec<(&str, String)> = files
                .iter()
                .map(|(p, b)| (p.as_str(), String::from_utf8_lossy(b).into_owned()))
                .collect();
            let files: Vec<(&str, &str)> = files.iter().map(|(p, t)| (*p, t.as_str())).collect();
            mover.commit(&files);
        }
    });
    let error = put_task(&scenario).unwrap_err();
    assert!(matches!(error, PutError::HeadMoved { .. }), "{error}");
    assert_eq!(error.code(), Code::PutHeadMoved);
}

#[test]
fn a_head_that_moves_on_every_attempt_refuses_after_three_retries_at_the_last_commit() {
    let scenario = seeded();
    let mover = Rc::clone(&scenario);
    let attempts = Rc::new(std::cell::Cell::new(0));
    let counted = Rc::clone(&attempts);
    let _hook = board_tree::before_head_moves(move |attempt| {
        counted.set(attempt + 1);
        commit_beside(&mover, &format!("other{attempt}.txt"));
    });
    let error = put_task(&scenario).unwrap_err();
    assert!(matches!(error, PutError::HeadMoved { .. }), "{error}");
    assert_eq!(attempts.get(), 4, "the first attempt and three retries");
    let head = scenario.head().unwrap();
    assert!(
        head.files.contains_key("other3.txt"),
        "HEAD is the last mover's"
    );
    assert!(
        !head.files.contains_key("tasks/a.json"),
        "the put committed nothing"
    );
}
