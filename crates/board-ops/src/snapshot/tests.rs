//! Deciding through the index: a warm index decides as a cold one, its views
//! show exactly what was decided, and it prunes what the tree left.

use board_index::{BlobFact, Index};
use board_model::{
    Change, Code, Kind, Location, Object, ObjectId, POINTER_TYPE, Role, Source, Stat,
};
use board_scenario::{Item, Scenario};
use board_tree::{Cache, Entry, Status};
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;
use serde_json::Value;

use crate::index::Indexed;
use crate::tests::generated::{Generated, generated};
use crate::tests::{board, open, task};
use crate::{CheckReport, Checked, Refreshed};

use super::{Facts, Indexing, known, read_type};

mod mounts;

/// Decides every entry of `scenario` through `index`, and returns what it
/// reported.
fn decided(scenario: &Scenario, index: &mut Index) -> CheckReport {
    open(scenario).snapshot(index, None).unwrap().report
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
/// when the last refresh began, which is a time, and the index's size after
/// its last prune.
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
    "SELECT * FROM state WHERE key NOT IN ('refreshed_ns', 'pruned_pages', 'prunes') ORDER BY key",
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
    index.trust("p", "1.0.0", "id").unwrap();
    index.handled("p", "1.0.0", "id", &blobs).unwrap();
}

#[test]
fn the_blobs_view_shows_what_the_tree_reaches_while_the_cache_keeps_the_rest() {
    let scenario = board();
    scenario.write("tasks/a.json", Item::File(&task("")));
    let cache = tempfile::tempdir().unwrap();
    let board = open(&scenario);
    let mut warm = Index::open(Some(cache.path()), board.repository().path()).unwrap();
    decided(&scenario, &mut warm);
    scenario.write(
        "tasks/a.json",
        Item::File(&task(r#", "see": "/projects/p""#)),
    );
    let mut cold = Index::open(None, board.repository().path()).unwrap();
    let warm = board.snapshot(&mut warm, None).unwrap();
    let cold = board.snapshot(&mut cold, None).unwrap();
    let count = |index: &Index, sql: &str| index.query(sql, 1).unwrap().rows[0][0].clone();
    let seen = "SELECT count(*) FROM blobs";
    assert_eq!(count(warm.index, seen), count(cold.index, seen));
    let cached = "SELECT count(*) FROM main.blobs";
    assert_eq!(
        count(warm.index, cached).as_i64().unwrap(),
        count(cold.index, cached).as_i64().unwrap() + 1,
        "the first version of tasks/a is still cached"
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

/// Deletes every fact the current tree does not reach.
fn prune(index: &mut Index) -> usize {
    let writer = index.write().unwrap();
    let pruned = writer.prune().unwrap();
    writer.commit().unwrap();
    pruned
}

/// Decides `scenario` through `warm` and through a cold index, extracting
/// everything after each, and requires them to agree: the same entries, the
/// same references, views that show those entries, and every table alike.
/// Returns what the warm index reported.
fn agrees(scenario: &Scenario, warm: &mut Index) -> Result<CheckReport, TestCaseError> {
    let board = open(scenario);
    let mut cold = Index::open(None, board.repository().path()).unwrap();
    let warm = board.snapshot(warm, None).unwrap();
    let cold = board.snapshot(&mut cold, None).unwrap();
    for snapshot in [&warm, &cold] {
        extract_everything(snapshot.index);
        prop_assert_eq!(
            viewed(snapshot.index),
            sorted(snapshot.report.entries.clone())
        );
    }
    prop_assert_eq!(&warm.report.entries, &cold.report.entries);
    prop_assert_eq!(&warm.references, &cold.references);
    for sql in EVERY_TABLE {
        prop_assert_eq!(
            warm.index.query(sql, 10_000).unwrap(),
            cold.index.query(sql, 10_000).unwrap(),
            "{}",
            sql
        );
    }
    Ok(warm.report)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    #[test]
    fn a_warm_index_decides_as_a_cold_one_after_a_change_and_after_a_prune(
        before in generated(),
        after in generated(),
        committed: bool,
    ) {
        let mut generated = Generated::new(r#"{"$type": "board/config/v1"}"#, &before, committed);
        let cache = tempfile::tempdir().unwrap();
        let root = open(generated.scenario()).repository().path().to_path_buf();
        let mut warm = Index::open(Some(cache.path()), &root).unwrap();
        extract_everything(open(generated.scenario()).snapshot(&mut warm, None).unwrap().index);
        generated.write(&after);

        agrees(generated.scenario(), &mut warm)?;
        let again = agrees(generated.scenario(), &mut warm)?;
        prop_assert_eq!(again.work, Refreshed::default(), "nothing changed");

        prune(&mut warm);
        generated.write(&before);
        agrees(generated.scenario(), &mut warm)?;
        prune(&mut warm);
        let again = agrees(generated.scenario(), &mut warm)?;
        prop_assert_eq!(again.work, Refreshed::default(), "a prune keeps what the tree reaches");
    }
}

/// A task whose title is `bytes` long, so that its facts fill many pages.
fn large_task(letter: char, bytes: usize) -> String {
    format!(
        r#"{{"$type": "lr/task/v1", "title": "{}"}}"#,
        letter.to_string().repeat(bytes)
    )
}

/// How many prunes `index` has run.
fn prunes(index: &Index) -> i64 {
    let sql =
        "SELECT CAST(coalesce(max(value), 0) AS INTEGER) FROM main.state WHERE key = 'prunes'";
    index.query(sql, 1).unwrap().rows[0][0].as_i64().unwrap()
}

/// Decides a board holding a large task, then again after the task is
/// replaced by `replacement`. Returns the rows and the prunes the second
/// run ran, and how many blobs the index then holds beyond those a cold
/// index holds.
fn replaced_by(replacement: &str) -> (usize, i64, i64) {
    let scenario = board();
    scenario.write("tasks/a.json", Item::File(&large_task('a', 200_000)));
    let cache = tempfile::tempdir().unwrap();
    let root = open(&scenario).repository().path().to_path_buf();
    let mut warm = Index::open(Some(cache.path()), &root).unwrap();
    let first = decided(&scenario, &mut warm);
    assert_eq!(first.work.pruned, 0);
    assert_eq!(prunes(&warm), 0, "a new index records its size instead");
    scenario.write("tasks/a.json", Item::File(replacement));
    let second = decided(&scenario, &mut warm);
    assert!(second.work.wrote);
    let mut cold = Index::open(None, &root).unwrap();
    decided(&scenario, &mut cold);
    let blobs = |index: &Index| {
        let rows = index.query("SELECT count(*) FROM main.blobs", 1).unwrap();
        rows.rows[0][0].as_i64().unwrap()
    };
    (
        second.work.pruned,
        prunes(&warm),
        blobs(&warm) - blobs(&cold),
    )
}

#[test]
fn a_snapshot_that_writes_once_the_index_has_doubled_prunes_what_the_tree_left() {
    let (deleted, prunes, extra) = replaced_by(&large_task('b', 400_000));
    // The first task's blob, body and validation.
    assert_eq!(deleted, 3);
    assert_eq!(prunes, 1);
    assert_eq!(extra, 0);
}

#[test]
fn a_snapshot_that_writes_before_the_index_has_doubled_keeps_what_the_tree_left() {
    let (deleted, prunes, extra) = replaced_by(&large_task('b', 10));
    assert_eq!(deleted, 0);
    assert_eq!(prunes, 0);
    assert_eq!(extra, 1, "the first task's blob is still cached");
}

#[test]
fn a_file_seen_as_it_is_now_is_known_and_any_other_is_counted_for_hashing() {
    let stat = Stat {
        size: 2,
        modified_ns: 1,
        changed_ns: 1,
        inode: 7,
        mode: 0o100_644,
    };
    let blob = ObjectId::from_hex("0123456789abcdef0123456789abcdef01234567").unwrap();
    let entries = [Entry {
        path: "a.json".to_owned(),
        logical: None,
        blob: Some(blob),
        status: Status::Ok,
        stat: Some(stat),
        size: Some(2),
    }];
    let mut hashed = 3;
    let mut known = known(&entries, &mut hashed);
    assert_eq!(known("a.json", &stat), Some(blob));
    let later = Stat {
        modified_ns: 2,
        ..stat
    };
    assert_eq!(known("a.json", &later), None);
    assert_eq!(known("b.json", &stat), None);
    drop(known);
    assert_eq!(hashed, 5, "two files left to hash after the three before");
}

#[test]
fn only_a_pointer_without_a_code_is_followed_as_a_mount() {
    let blob = ObjectId::from_hex("0123456789abcdef0123456789abcdef01234567").unwrap();
    let indexed = |kind, code| Indexed {
        blob,
        role: Role::Json,
        fact: BlobFact {
            kind,
            record_type: None,
            code,
        },
    };
    let facts: Facts = [
        ("pointer.json", indexed(Kind::Pointer, None)),
        (
            "broken.json",
            indexed(Kind::Pointer, Some(Code::MountInvalid)),
        ),
        ("record.json", indexed(Kind::Record, None)),
    ]
    .into_iter()
    .map(|(path, indexed)| (path.to_owned(), indexed))
    .collect();
    let followed: Vec<&str> = ["pointer.json", "broken.json", "record.json", "absent.json"]
        .into_iter()
        .filter(|path| <Indexing<'_, '_> as Cache>::is_pointer(&facts, path))
        .collect();
    assert_eq!(followed, ["pointer.json"]);
}

#[test]
fn a_pointer_the_index_does_not_hold_is_read_as_the_pointer_type_and_a_record_as_its_own() {
    let scenario = board();
    let link = r#"{"$type": "board/mount/v1", "path": "/tasks/a"}"#;
    scenario
        .write("tasks/a.json", Item::File(&task("")))
        .write("l.json", Item::File(link));
    let board = open(&scenario);
    let at = |path: &str, text: &str| {
        let blob = scenario.blob(text);
        let location = Location {
            board: board_model::Board {
                repository: board.repository().path().to_path_buf(),
                source: Source::WorkingTree,
            },
            path: path.to_owned(),
            object: Object::Blob(blob),
            pointers: Vec::new(),
        };
        read_type(board.repositories(), &location, blob)
    };
    assert_eq!(at("l.json", link), POINTER_TYPE.parse().ok());
    assert_eq!(at("tasks/a.json", &task("")), "lr/task/v1".parse().ok());
}
