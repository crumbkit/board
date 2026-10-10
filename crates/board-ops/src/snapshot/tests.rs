//! Deciding through the index: a warm index decides as a cold one, its views
//! show exactly what was decided, and it prunes what the tree left.

use board_index::{BlobFact, Index, View};
use board_model::{
    Change, Code, Kind, Location, Object, ObjectId, POINTER_TYPE, Role, Source, Stat,
};
use board_scenario::{Item, Scenario};
use board_tree::{Cache, Entry, Status};
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;
use serde_json::Value;

use crate::index::Indexed;
use crate::tests::{board, open, task};
use crate::{CheckReport, Checked, Refreshed};
use board_scenario::generated::{Generated, generated};

use super::{Decide, Facts, Indexing, Referenced, Snapshot, known, read_type};

/// A snapshot with its views filled, as a query reads it.
struct Filled {
    view: View,
    report: CheckReport,
    references: Vec<Referenced>,
}

/// Decides every entry of `board` through `index` and fills the views.
fn filled(board: &crate::Board, index: Index) -> Filled {
    let Snapshot {
        decided,
        report,
        references,
        ..
    } = board.snapshot(index, None).unwrap();
    Filled {
        view: board.view(decided).unwrap(),
        report,
        references,
    }
}

mod mounts;

/// How many validations and how many blob facts deciding `scenario`
/// through the index in `cache` looks up.
fn looked_up(scenario: &Scenario, cache: &std::path::Path) -> (usize, usize) {
    let board = open(scenario);
    let index = Index::open(Some(cache), board.repository().path()).unwrap();
    let (read, _, _) = crate::index::transact(index, |writer| {
        let (_, entries) = board.decide(writer, None)?;
        Ok(((writer.validations_read(), writer.blobs_read()), entries))
    })
    .unwrap();
    read
}

/// How many validations deciding `scenario` through the index in `cache`
/// looks up.
fn validations_read(scenario: &Scenario, cache: &std::path::Path) -> usize {
    looked_up(scenario, cache).0
}

/// How many blob facts a check of three tasks looks up, cold and then warm,
/// when each task holds `fields`.
fn blobs_read(fields: &str) -> [usize; 2] {
    let scenario = board();
    for name in ["a", "b", "c"] {
        // Each task is distinct content, so each is one blob.
        let fields = format!(r#", "note": "{name}"{fields}"#);
        scenario.write(&format!("tasks/{name}.json"), Item::File(&task(&fields)));
    }
    let cache = tempfile::tempdir().unwrap();
    [(); 2].map(|()| looked_up(&scenario, cache.path()).1)
}

#[test]
fn following_a_reference_to_an_entry_of_the_board_looks_up_no_blob() {
    let referring = blobs_read(r#", "project": "/projects/p""#);
    let plain = blobs_read("");
    assert_eq!(referring, plain, "[cold, warm] with and without references");
}

#[test]
fn a_check_looks_up_each_records_validation_once_cold_and_warm() {
    let scenario = board();
    scenario
        .write("tasks/a.json", Item::File(&task("")))
        .write("tasks/b.json", Item::File(&task("")))
        .write("tasks/bad.json", Item::File(r#"{"$type": "lr/task/v1"}"#));
    let cache = tempfile::tempdir().unwrap();
    // The project and three tasks are checked against a schema.
    assert_eq!(validations_read(&scenario, cache.path()), 4, "cold");
    assert_eq!(validations_read(&scenario, cache.path()), 4, "warm");
}

/// Decides every entry of `scenario` through `index`, and returns what it
/// reported.
fn decided(scenario: &Scenario, index: Index) -> CheckReport {
    open(scenario).snapshot(index, None).unwrap().report
}

/// What `entries` shows, as check reports it.
fn viewed(view: &View) -> Vec<Checked> {
    let rows = view
        .query(
            "SELECT path, logical, kind, code, change, status, source FROM entries ORDER BY source, path",
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
                schema: None,
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
fn extract_everything(view: &View) {
    let types = view
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
    let blobs: Vec<_> = view
        .unhandled("p", "1.0.0", &types)
        .unwrap()
        .into_iter()
        .map(|unhandled| unhandled.blob)
        .collect();
    view.trust("p", "1.0.0", "id").unwrap();
    view.handled("p", "1.0.0", "id", &blobs).unwrap();
}

#[test]
fn the_blobs_view_shows_what_the_tree_reaches_while_the_cache_keeps_the_rest() {
    let scenario = board();
    scenario.write("tasks/a.json", Item::File(&task("")));
    let cache = tempfile::tempdir().unwrap();
    let board = open(&scenario);
    let warm = || Index::open(Some(cache.path()), board.repository().path()).unwrap();
    decided(&scenario, warm());
    scenario.write(
        "tasks/a.json",
        Item::File(&task(r#", "see": "/projects/p""#)),
    );
    let cold = Index::open(None, board.repository().path()).unwrap();
    let warm = filled(&board, warm());
    let cold = filled(&board, cold);
    let count = |view: &View, sql: &str| view.query(sql, 1).unwrap().rows[0][0].clone();
    let seen = "SELECT count(*) FROM blobs";
    assert_eq!(count(&warm.view, seen), count(&cold.view, seen));
    let cached = "SELECT count(*) FROM main.blobs";
    assert_eq!(
        count(&warm.view, cached).as_i64().unwrap(),
        count(&cold.view, cached).as_i64().unwrap() + 1,
        "the first version of tasks/a is still cached"
    );
}

/// Entries in the order the view's key gives them, by source and then path,
/// without the detail and schema file the view does not show.
fn sorted(mut entries: Vec<Checked>) -> Vec<Checked> {
    entries.sort_by_key(|entry| (board_index::source_name(&entry.source), entry.path.clone()));
    for entry in &mut entries {
        entry.detail = None;
        entry.schema = None;
    }
    entries
}

/// Deletes every fact the current tree does not reach.
fn prune(index: Index) -> usize {
    let writer = index.write().unwrap();
    let pruned = writer.prune().unwrap();
    writer.decide(Vec::new()).unwrap();
    pruned
}

/// Decides `scenario` through `warm` and through a cold index, extracting
/// everything after each, and requires them to agree: the same entries, the
/// same references, views that show those entries, and every table alike.
/// Returns what the warm index reported.
fn agrees(scenario: &Scenario, warm: Index) -> Result<CheckReport, TestCaseError> {
    let board = open(scenario);
    let cold = Index::open(None, board.repository().path()).unwrap();
    let warm = filled(&board, warm);
    let cold = filled(&board, cold);
    for snapshot in [&warm, &cold] {
        extract_everything(&snapshot.view);
        prop_assert_eq!(
            viewed(&snapshot.view),
            sorted(snapshot.report.entries.clone())
        );
    }
    prop_assert_eq!(&warm.report.entries, &cold.report.entries);
    prop_assert_eq!(&warm.references, &cold.references);
    for sql in EVERY_TABLE {
        prop_assert_eq!(
            warm.view.query(sql, 10_000).unwrap(),
            cold.view.query(sql, 10_000).unwrap(),
            "{}",
            sql
        );
    }
    Ok(warm.report)
}

/// Decides the board `before` describes warm, changes it to `after`, and
/// requires a warm index to decide as a cold one, again after a prune, and
/// to do no work when nothing changed.
fn warm_decides_as_cold(
    before: &[Option<usize>],
    after: &[Option<usize>],
    committed: bool,
) -> Result<(), TestCaseError> {
    let mut generated = Generated::new(r#"{"$type": "board/config/v1"}"#, before, committed);
    let cache = tempfile::tempdir().unwrap();
    let root = open(generated.scenario()).repository().path().to_path_buf();
    let warm = || Index::open(Some(cache.path()), &root).unwrap();
    extract_everything(&filled(&open(generated.scenario()), warm()).view);
    generated.write(after);

    agrees(generated.scenario(), warm())?;
    let again = agrees(generated.scenario(), warm())?;
    prop_assert_eq!(again.work, Refreshed::default(), "nothing changed");

    prune(warm());
    generated.write(before);
    agrees(generated.scenario(), warm())?;
    prune(warm());
    let again = agrees(generated.scenario(), warm())?;
    prop_assert_eq!(
        again.work,
        Refreshed::default(),
        "a prune keeps what the tree reaches"
    );
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    #[test]
    fn a_warm_index_decides_as_a_cold_one_after_a_change_and_after_a_prune(
        before in generated(),
        after in generated(),
        committed: bool,
    ) {
        warm_decides_as_cold(&before, &after, committed)?;
    }
}

#[test]
fn a_warm_index_shows_no_body_for_a_blob_that_was_a_record_and_is_now_a_file_without_a_logical_path()
 {
    let mut task_file = [None; 9];
    task_file[7] = Some(2);
    let mut task_project = [None; 9];
    task_project[2] = Some(1);
    warm_decides_as_cold(&task_file, &task_project, false).unwrap();
}

#[test]
fn a_warm_index_shows_no_body_for_a_blob_that_was_a_record_and_is_now_a_schema() {
    let mut task_schema = [None; 9];
    task_schema[0] = Some(3);
    let mut task_project = [None; 9];
    task_project[2] = Some(1);
    warm_decides_as_cold(&task_schema, &task_project, false).unwrap();
}

#[test]
fn a_warm_index_lists_no_tree_again_when_heads_root_tree_is_empty() {
    let nothing = [None; 8];
    warm_decides_as_cold(&nothing, &nothing, true).unwrap();
}

#[test]
fn a_warm_index_lists_no_tree_again_when_heads_root_tree_holds_a_file() {
    let mut notes = [None; 8];
    notes[6] = Some(0);
    warm_decides_as_cold(&notes, &notes, true).unwrap();
}

/// A task whose title is `bytes` long, so that its facts fill many pages.
fn large_task(letter: char, bytes: usize) -> String {
    format!(
        r#"{{"$type": "lr/task/v1", "title": "{}"}}"#,
        letter.to_string().repeat(bytes)
    )
}

/// How many prunes `index` has run.
fn prunes(view: &View) -> i64 {
    let sql =
        "SELECT CAST(coalesce(max(value), 0) AS INTEGER) FROM main.state WHERE key = 'prunes'";
    view.query(sql, 1).unwrap().rows[0][0].as_i64().unwrap()
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
    let warm = || Index::open(Some(cache.path()), &root).unwrap();
    let first = filled(&open(&scenario), warm());
    assert_eq!(first.report.work.pruned, 0);
    assert_eq!(
        prunes(&first.view),
        0,
        "a new index records its size instead"
    );
    drop(first);
    scenario.write("tasks/a.json", Item::File(replacement));
    let second = filled(&open(&scenario), warm());
    assert!(second.report.work.wrote);
    let cold = Index::open(None, &root).unwrap();
    let cold = filled(&open(&scenario), cold);
    let blobs = |view: &View| {
        let rows = view.query("SELECT count(*) FROM main.blobs", 1).unwrap();
        rows.rows[0][0].as_i64().unwrap()
    };
    (
        second.report.work.pruned,
        prunes(&second.view),
        blobs(&second.view) - blobs(&cold.view),
    )
}

#[test]
fn a_snapshot_that_writes_once_the_index_has_doubled_prunes_what_the_tree_left() {
    let (deleted, prunes, extra) = replaced_by(&large_task('b', 800_000));
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
fn a_file_seen_as_it_is_now_is_known_or_tried_again_uncounted_and_any_other_is_counted() {
    let stat = Stat {
        size: 2,
        modified_ns: 1,
        changed_ns: 1,
        inode: 7,
        mode: 0o100_644,
    };
    let blob = ObjectId::from_hex("0123456789abcdef0123456789abcdef01234567").unwrap();
    let entry = |path: &str, blob| Entry {
        path: path.to_owned(),
        logical: None,
        blob,
        status: Status::Ok,
        stat: Some(stat),
        size: Some(2),
    };
    let entries = [entry("a.json", Some(blob)), entry("sealed.json", None)];
    let mut hashed = 3;
    let mut known = known(&entries, &mut hashed);
    assert_eq!(known("a.json", &stat), Some(blob));
    assert_eq!(known("sealed.json", &stat), None);
    let later = Stat {
        modified_ns: 2,
        ..stat
    };
    assert_eq!(known("a.json", &later), None);
    assert_eq!(known("sealed.json", &later), None);
    assert_eq!(known("b.json", &stat), None);
    drop(known);
    assert_eq!(
        hashed, 6,
        "two changed files and one unseen after the three before"
    );
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
            reason: None,
        },
        validation: None,
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
        .filter(|path| <Indexing<'_> as Cache>::is_pointer(&facts, path))
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

#[test]
fn a_record_whose_validation_the_index_lacks_cannot_be_checked_and_one_it_holds_is_ok() {
    let scenario = board();
    let board = open(&scenario);
    let index = Index::open(None, board.repository().path()).unwrap();
    let writer = index.write().unwrap();
    let facts = std::collections::HashMap::new();
    let decide = Decide {
        repositories: board.repositories(),
        writer: &writer,
        listed: &[],
        facts: &facts,
        mounted: None,
        resolve: std::cell::RefCell::new(Box::new(|_: &board_model::LogicalPath| {
            panic!("a record with no references resolves nothing")
        })),
    };
    let blob = ObjectId::from_hex("0123456789abcdef0123456789abcdef01234567").unwrap();
    let record = |validation| Indexed {
        blob,
        role: Role::Json,
        fact: BlobFact {
            kind: Kind::Record,
            record_type: Some("lr/task/v1".parse().unwrap()),
            code: None,
            reason: None,
        },
        validation,
    };
    let record_type: board_model::RecordType = "lr/task/v1".parse().unwrap();
    let judged = |indexed: &Indexed| {
        decide
            .record(indexed, Some((&record_type, Ok(blob))), &mut Vec::new())
            .unwrap()
            .unwrap()
    };
    let (code, detail) = judged(&record(None)).unwrap();
    assert_eq!(code, Code::RepoUnreadable);
    assert_eq!(
        detail.unwrap().reason,
        "cannot be checked: it or its schema changed or could not be read; run the command again"
    );
    let valid = record(Some(board_index::ValidationFact::Valid(Vec::new())));
    assert_eq!(judged(&valid), None);
}
