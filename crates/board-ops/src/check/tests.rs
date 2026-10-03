//! Checking through the index: what it says against HEAD, and that a warm
//! index answers as a cold one and as its own `entries` view.

use std::collections::BTreeMap;

use board_index::Index;
use board_model::{Change, Code, Kind};
use board_scenario::{Item, Scenario};
use board_tree::Status;
use proptest::prelude::*;
use serde_json::Value;

use crate::tests::{PROJECT_SCHEMA, TASK_SCHEMA, board, open, task};
use crate::{CheckReport, Checked};

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

/// What `entries` shows, as check reports it.
fn viewed(index: &Index) -> Vec<Checked> {
    let rows = index
        .query("SELECT path, logical, kind, code, change, status FROM entries ORDER BY path")
        .unwrap();
    let text = |value: &Value| value.as_str().map(str::to_owned);
    rows.rows
        .iter()
        .map(|row| {
            let code = text(&row[3]).map(|code| Code::parse(&code).unwrap());
            assert_eq!(
                text(&row[5]).unwrap(),
                if code.is_some() { "degraded" } else { "ok" }
            );
            Checked {
                path: text(&row[0]).unwrap(),
                logical: text(&row[1]).map(|logical| logical.parse().unwrap()),
                kind: Kind::ALL
                    .into_iter()
                    .find(|kind| Some(kind.as_str()) == row[2].as_str())
                    .unwrap(),
                status: code.map_or(Status::Ok, Status::Degraded),
                change: if row[4].as_str() == Some("committed") {
                    Change::Committed
                } else {
                    Change::Uncommitted
                },
            }
        })
        .collect()
}

/// The files a generated board can hold, and what each may contain.
fn variants() -> Vec<(&'static str, Vec<String>)> {
    let link = |target: &str| format!(r#"{{"$type": "board/mount/v1", "path": "{target}"}}"#);
    vec![
        (
            ".board/types/lr/task/v1.schema.json",
            vec![TASK_SCHEMA.to_owned(), r#"{"type": 7}"#.to_owned()],
        ),
        (
            ".board/types/lr/project/v1.schema.json",
            vec![PROJECT_SCHEMA.to_owned()],
        ),
        (
            "projects/p.json",
            vec![
                r#"{"$type": "lr/project/v1"}"#.to_owned(),
                r#"{"$type": "lr/task/v1", "title": "t"}"#.to_owned(),
            ],
        ),
        (
            "tasks/a.json",
            vec![
                task(r#", "project": "/projects/p""#),
                task(r#", "see": "/nowhere""#),
                r#"{"$type": "lr/task/v1"}"#.to_owned(),
                "{".to_owned(),
            ],
        ),
        (
            "tasks/b.json",
            vec![
                task(r#", "see": "/tasks/a""#),
                task(r#", "project": "/links/l""#),
            ],
        ),
        (
            "links/l.json",
            vec![link("/projects/p"), link("/gone"), link("/links/l")],
        ),
        ("notes.md", vec!["a".to_owned(), "b".to_owned()]),
    ]
}

/// For each file in [`variants`], which content it holds, if any.
fn generated() -> impl Strategy<Value = Vec<Option<usize>>> {
    let choices: Vec<_> = variants()
        .into_iter()
        .map(|(_, contents)| prop::option::of(0..contents.len()))
        .collect();
    choices
}

/// Writes the generated board over whatever the working tree holds.
fn write(scenario: &Scenario, choice: &[Option<usize>], present: &mut BTreeMap<String, bool>) {
    for ((path, contents), chosen) in variants().into_iter().zip(choice) {
        let was = present.insert(path.to_owned(), chosen.is_some()) == Some(true);
        match chosen {
            Some(at) => {
                scenario.write(path, Item::File(&contents[*at]));
            }
            None if was => {
                scenario.write(path, Item::Deleted);
            }
            None => {}
        }
    }
}

fn check_in(scenario: &Scenario, index: &mut Index) -> CheckReport {
    open(scenario).check_in(index, None).unwrap()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    #[test]
    fn a_warm_index_checks_as_a_cold_one_and_its_view_agrees(
        before in generated(),
        after in generated(),
        committed: bool,
    ) {
        let scenario = Scenario::new();
        scenario.write(".board/board.json", Item::File(r#"{"$type": "board/config/v1"}"#));
        let mut present = BTreeMap::new();
        write(&scenario, &before, &mut present);
        if committed {
            let files: Vec<(String, String)> = variants()
                .into_iter()
                .zip(&before)
                .filter_map(|((path, contents), chosen)| {
                    Some((path.to_owned(), contents[(*chosen)?].clone()))
                })
                .collect();
            let files: Vec<(&str, &str)> =
                files.iter().map(|(path, text)| (path.as_str(), text.as_str())).collect();
            scenario.commit(&files);
        }
        let cache = tempfile::tempdir().unwrap();
        let root = open(&scenario).repository().path().to_path_buf();
        let mut warm = Index::open(Some(cache.path()), &root).unwrap();
        check_in(&scenario, &mut warm);
        write(&scenario, &after, &mut present);

        let warmed = check_in(&scenario, &mut warm);
        let mut cold = Index::open(None, &root).unwrap();
        let fresh = check_in(&scenario, &mut cold);
        prop_assert_eq!(&warmed.entries, &fresh.entries);
        prop_assert_eq!(viewed(&warm), warmed.entries.clone());
        prop_assert_eq!(viewed(&cold), fresh.entries);
    }
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
