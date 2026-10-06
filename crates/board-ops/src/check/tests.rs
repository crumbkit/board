//! Checking through the index: what it says against HEAD, and that a warm
//! index answers as a cold one and as its own `entries` view.

use board_index::Index;
use board_model::{Change, Code, Kind, ObjectId, Source};
use board_scenario::{Item, Scenario};
use board_tree::Status;
use proptest::prelude::*;
use serde_json::Value;

use crate::tests::generated::{Generated, generated};
use crate::tests::{PROJECT_SCHEMA, TASK_SCHEMA, board, open, task};
use crate::{CheckReport, Checked, Refreshed};

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
        .query(
            "SELECT path, logical, kind, code, change, status, source, type FROM entries ORDER BY source, path",
            10_000,
        )
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
                source: match text(&row[6]).unwrap().as_str() {
                    "worktree" => Source::WorkingTree,
                    commit => Source::Commit(ObjectId::from_hex(commit).unwrap()),
                },
                path: text(&row[0]).unwrap(),
                logical: text(&row[1]).map(|logical| logical.parse().unwrap()),
                kind: Kind::ALL
                    .into_iter()
                    .find(|kind| Some(kind.as_str()) == row[2].as_str())
                    .unwrap(),
                status: code.map_or(Status::Ok, Status::Degraded),
                detail: None,
                change: if row[4].as_str() == Some("committed") {
                    Change::Committed
                } else {
                    Change::Uncommitted
                },
                schema: text(&row[7])
                    .filter(|record_type| {
                        // Board's own types never have a schema file.
                        code == Some(Code::RecordUnknownType) && !record_type.starts_with("board/")
                    })
                    .map(|record_type| format!(".board/types/{record_type}.schema.json")),
            }
        })
        .collect()
}

/// Every table and view an agent can name, read whole. `state` leaves out
/// when the last refresh began, which is a time.
const EVERY_TABLE: [&str; 12] = [
    "SELECT * FROM entries ORDER BY source, path",
    "SELECT * FROM records ORDER BY source, path",
    "SELECT * FROM bodies ORDER BY blob",
    "SELECT * FROM blobs ORDER BY blob, role",
    "SELECT * FROM validations ORDER BY record, schema",
    "SELECT * FROM refs ORDER BY record, schema, ordinal",
    "SELECT * FROM trees ORDER BY tree, name",
    "SELECT * FROM worktree ORDER BY path",
    "SELECT * FROM sources ORDER BY source",
    "SELECT * FROM mounts ORDER BY source, path",
    "SELECT * FROM state WHERE key <> 'refreshed_ns' ORDER BY key",
    "SELECT * FROM extracted ORDER BY plugin, version, blob",
];

/// Marks every record the index would send a plugin extracting every type
/// as handled, as a plugin naming every blob it was sent does.
fn extract_everything(index: &Index) {
    let types = index
        .query(
            "SELECT DISTINCT type FROM entries WHERE type IS NOT NULL",
            10_000,
        )
        .unwrap();
    let types: Vec<&str> = types
        .rows
        .iter()
        .filter_map(|row| row[0].as_str())
        .collect();
    let blobs: Vec<_> = index
        .unhandled("p", "1.0.0", &types)
        .unwrap()
        .into_iter()
        .map(|unhandled| unhandled.blob)
        .collect();
    index.handled("p", "1.0.0", &blobs).unwrap();
}

#[test]
fn a_query_sees_what_the_tree_reaches_while_the_cache_keeps_the_rest() {
    let scenario = board();
    scenario.write("tasks/a.json", Item::File(&task("")));
    let cache = tempfile::tempdir().unwrap();
    let root = open(&scenario).repository().path().to_path_buf();
    let mut warm = Index::open(Some(cache.path()), &root).unwrap();
    check_in(&scenario, &mut warm);
    scenario.write(
        "tasks/a.json",
        Item::File(&task(r#", "see": "/projects/p""#)),
    );
    check_in(&scenario, &mut warm);
    let mut cold = Index::open(None, &root).unwrap();
    check_in(&scenario, &mut cold);
    let count = |index: &Index, sql: &str| index.query(sql, 1).unwrap().rows[0][0].clone();
    let seen = "SELECT count(*) FROM blobs";
    assert_eq!(count(&warm, seen), count(&cold, seen));
    let cached = "SELECT count(*) FROM main.blobs";
    assert_eq!(
        count(&warm, cached).as_i64().unwrap(),
        count(&cold, cached).as_i64().unwrap() + 1,
        "the first version of tasks/a is still cached"
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
    let mut other = Index::open(Some(cache.path()), board.repository().path()).unwrap();
    let writing = other.write().unwrap();
    let warm = board.check(None, Some(cache.path())).unwrap();
    assert_eq!(warm.memory, None, "it used the file without waiting");
    assert_eq!(warm.work, Refreshed::default());
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
            validated: 1,
            wrote: true,
            ..Refreshed::default()
        }
    );
}

/// Entries in the order the view's key gives them, by source and then path,
/// without the detail the view does not show.
fn sorted(mut entries: Vec<Checked>) -> Vec<Checked> {
    entries.sort_by_key(|entry| (board_index::source_name(&entry.source), entry.path.clone()));
    for entry in &mut entries {
        entry.detail = None;
    }
    entries
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
        let mut generated = Generated::new(r#"{"$type": "board/config/v1"}"#, &before, committed);
        let cache = tempfile::tempdir().unwrap();
        let root = open(generated.scenario()).repository().path().to_path_buf();
        let mut warm = Index::open(Some(cache.path()), &root).unwrap();
        check_in(generated.scenario(), &mut warm);
        extract_everything(&warm);
        generated.write(&after);

        let warmed = check_in(generated.scenario(), &mut warm);
        extract_everything(&warm);
        let again = check_in(generated.scenario(), &mut warm);
        prop_assert_eq!(again.work, Refreshed::default(), "nothing changed");
        let mut cold = Index::open(None, &root).unwrap();
        let fresh = check_in(generated.scenario(), &mut cold);
        extract_everything(&cold);
        prop_assert_eq!(&warmed.entries, &fresh.entries);
        prop_assert_eq!(&again.entries, &fresh.entries);
        prop_assert_eq!(viewed(&warm), sorted(warmed.entries.clone()));
        prop_assert_eq!(viewed(&cold), sorted(fresh.entries));
        for sql in EVERY_TABLE {
            prop_assert_eq!(warm.query(sql, 10_000).unwrap(), cold.query(sql, 10_000).unwrap(), "{}", sql);
        }
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
