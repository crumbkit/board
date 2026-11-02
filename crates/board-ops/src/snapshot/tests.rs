//! Deciding through the index: a warm index decides as a cold one, its views
//! show exactly what was decided, and it prunes what the tree left.

use board_index::{BlobFact, Index, Rows, View};
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

use super::{Decide, Facts, Indexing, Need, Referenced, Snapshot, known};

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
    } = board.snapshot(index, None, Need::View).unwrap();
    Filled {
        view: board.view(decided).unwrap(),
        report,
        references,
    }
}

mod cache;
mod decisions;
mod mounts;
mod scoped;

/// How many validations and how many blob facts deciding `scenario`
/// through the index in `cache` looks up.
fn looked_up(scenario: &Scenario, cache: &std::path::Path) -> (usize, usize) {
    let board = open(scenario);
    let index = Index::open(Some(cache), board.repository().path()).unwrap();
    let (read, _, _) = crate::index::transact(index, |writer| {
        let (_, entries) = board.decide(writer, None, Need::Report)?;
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
    open(scenario)
        .snapshot(index, None, Need::Report)
        .unwrap()
        .report
}

/// What `entries` shows, as check reports it.
fn viewed(view: &View) -> Vec<Checked> {
    let sql = "SELECT path, logical, kind, code, change, status, source FROM entries \
               ORDER BY source, path";
    let rows = view.query(sql, 10_000, None).unwrap();
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
const EVERY_TABLE: [&str; 15] = [
    "SELECT * FROM entries ORDER BY source, path",
    "SELECT * FROM entries",
    "SELECT * FROM decided ORDER BY key, path",
    "SELECT * FROM pointers ORDER BY scan, path",
    "SELECT * FROM records ORDER BY source, path",
    "SELECT * FROM bodies ORDER BY blob",
    "SELECT * FROM blobs ORDER BY blob, role",
    "SELECT * FROM validations ORDER BY record, schema",
    "SELECT * FROM refs ORDER BY record, schema, ordinal",
    "SELECT * FROM links ORDER BY blob, ordinal",
    "SELECT * FROM trees ORDER BY tree, name",
    "SELECT * FROM worktree ORDER BY path",
    "SELECT * FROM sources ORDER BY source",
    "SELECT * FROM mounts ORDER BY source, path",
    "SELECT * FROM state WHERE key NOT IN ('refreshed_ns', 'pruned_pages', 'prunes') ORDER BY key",
];

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
    let count = |view: &View, sql: &str| view.query(sql, 1, None).unwrap().rows[0][0].clone();
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

/// Decides `scenario` through an index `warm` opens and through a cold one,
/// first for views, as a query does, then for a report alone, as a check
/// does, then for a report and references, as refs does, and requires them to agree: the
/// same entries in the same order, with their detail, the same references,
/// views that show those entries, and every table alike. Returns what the
/// warm index reported for refs.
fn agrees(scenario: &Scenario, warm: &dyn Fn() -> Index) -> Result<CheckReport, TestCaseError> {
    let board = open(scenario);
    let cold = || Index::open(None, board.repository().path()).unwrap();
    let (warm_view, cold_view) = (filled(&board, warm()), filled(&board, cold()));
    let checked = [warm(), cold()].map(|index| {
        let snapshot = board.snapshot(index, None, Need::Check).unwrap();
        snapshot.report.entries
    });
    let [warm, cold] = [warm(), cold()].map(|index| {
        let snapshot = board.snapshot(index, None, Need::Report).unwrap();
        (snapshot.report, snapshot.references)
    });
    for (view, report) in [(&warm_view.view, &warm.0), (&cold_view.view, &cold.0)] {
        prop_assert_eq!(viewed(view), sorted(report.entries.clone()));
    }
    prop_assert_eq!(&warm.0.entries, &cold.0.entries);
    prop_assert_eq!(&warm.1, &cold.1);
    for checked in &checked {
        prop_assert_eq!(checked, &cold.0.entries);
    }
    let (warm_view, cold_view) = (warm_view.view, cold_view.view);
    let warm = warm.0;
    for sql in EVERY_TABLE {
        prop_assert_eq!(
            warm_view.query(sql, 10_000, None).unwrap(),
            cold_view.query(sql, 10_000, None).unwrap(),
            "{}",
            sql
        );
    }
    Ok(warm)
}

/// Checks the mounted library, and only its tasks, of `scenario` through
/// `warm` and through a cold index, for a report and for a report alone,
/// and requires them to agree, failing or not.
fn scoped_agrees(scenario: &Scenario, warm: &dyn Fn() -> Index) -> Result<(), TestCaseError> {
    let board = open(scenario);
    let code = |error: crate::CheckError| error.code();
    for scope in ["/vendor", "/vendor/tasks"] {
        let scope = scope.parse().unwrap();
        let [reported, cold] = [warm(), cold_index(&board)].map(|index| {
            let snapshot = board.snapshot(index, Some(&scope), Need::Report);
            snapshot.map(|snapshot| (snapshot.report.entries, snapshot.references))
        });
        let cold = cold.map_err(code);
        prop_assert_eq!(reported.map_err(code), cold.clone());
        for index in [warm(), cold_index(&board)] {
            let snapshot = board.snapshot(index, Some(&scope), Need::Check);
            let entries = snapshot.map(|snapshot| snapshot.report.entries);
            let expected = cold.clone().map(|(entries, _)| entries);
            prop_assert_eq!(entries.map_err(code), expected);
        }
    }
    Ok(())
}

/// A cold index for `board`.
fn cold_index(board: &crate::Board) -> Index {
    Index::open(None, board.repository().path()).unwrap()
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
    filled(&open(generated.scenario()), warm());
    generated.write(after);

    agrees(generated.scenario(), &warm)?;
    let again = agrees(generated.scenario(), &warm)?;
    prop_assert_eq!(again.work, decided_only(again.work), "nothing changed");
    scoped_agrees(generated.scenario(), &warm)?;

    prune(warm());
    generated.write(before);
    agrees(generated.scenario(), &warm)?;
    prune(warm());
    let again = agrees(generated.scenario(), &warm)?;
    // A prune outside a run reaches no decision, so the next run keeps
    // each one again.
    let wrote = Refreshed {
        wrote: false,
        ..again.work
    };
    prop_assert_eq!(
        wrote,
        decided_only(again.work),
        "a prune keeps what the tree reaches"
    );
    Ok(())
}

/// The counts of `work` that every snapshot does again, because whether a
/// pointer resolves depends on the whole tree and a check walks each source
/// it decides from the index; nothing else.
fn decided_only(work: Refreshed) -> Refreshed {
    Refreshed {
        pointers_read: work.pointers_read,
        repositories: work.repositories,
        walked: work.walked,
        ..Refreshed::default()
    }
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

/// Decides `scenario` through a cold index, hashing on `threads` threads:
/// what it reported, its references, and every table.
fn built_on(scenario: &Scenario, threads: usize) -> (CheckReport, Vec<Referenced>, Vec<Rows>) {
    let board = open(scenario);
    board
        .repository()
        .set_threads(std::num::NonZeroUsize::new(threads).unwrap());
    let cold = Index::open(None, board.repository().path()).unwrap();
    let snapshot = board.snapshot(cold, None, Need::Report).unwrap();
    let view = board.view(snapshot.decided).unwrap();
    let tables = EVERY_TABLE
        .iter()
        .map(|sql| view.query(sql, 10_000, None).unwrap())
        .collect();
    (snapshot.report, snapshot.references, tables)
}

#[test]
fn a_cold_index_built_on_one_thread_is_the_cold_index_built_on_four() {
    let scenario = board();
    for n in 0..30 {
        let text = task(&format!(r#", "n": {n}, "see": "/projects/p""#));
        scenario.write(&format!("tasks/t{}/n{n}.json", n % 3), Item::File(&text));
    }
    scenario
        .write("tasks/twin.json", Item::File(&task("")))
        .write("tasks/twin2.json", Item::File(&task("")))
        .write("tasks/bad.json", Item::File("{bad"))
        .write("tasks/locked.json", Item::Unreadable("{}"))
        .write("notes.md", Item::File("not a record"));
    let one = built_on(&scenario, 1);
    let four = built_on(&scenario, 4);
    assert_eq!(one.0.work, four.0.work, "the same work is counted");
    assert_eq!(one, four);
    assert_eq!(
        one.0.work.hashed, 39,
        "every file, the unreadable one included"
    );
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
    view.query(sql, 1, None).unwrap().rows[0][0]
        .as_i64()
        .unwrap()
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
        let rows = view
            .query("SELECT count(*) FROM main.blobs", 1, None)
            .unwrap();
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
        link: None,
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
            links: Vec::new(),
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
            panic!("reading a type resolves nothing")
        })),
        reader: std::cell::RefCell::new(board.repository().reader()),
        failed: std::cell::Cell::new(false),
        remotes: std::cell::OnceCell::new(),
    };
    let at = |path: &str, text: &str| {
        let location = Location {
            board: board_model::Board {
                repository: board.repository().path().to_path_buf(),
                source: Source::WorkingTree,
            },
            path: path.to_owned(),
            object: Object::Blob(scenario.blob(text)),
            pointers: Vec::new(),
        };
        decide.record_type(&location).unwrap()
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
        reader: std::cell::RefCell::new(board.repository().reader()),
        failed: std::cell::Cell::new(false),
        remotes: std::cell::OnceCell::new(),
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
            links: Vec::new(),
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

#[test]
fn a_snapshot_mounts_then_decides_then_commits_within_its_own_time() {
    let scenario = board();
    let board = open(&scenario);
    board.check(None, None).unwrap();
    let trace = board.take_trace();
    let phase = |name: &str| {
        let phase = trace.phases.iter().find(|phase| phase.name == name);
        phase.map(|phase| (phase.started, phase.started + phase.took))
    };
    let (start, end) = phase("snapshot").unwrap();
    let parts =
        ["snapshot.mounts", "snapshot.decide", "snapshot.commit"].map(|name| phase(name).unwrap());
    for pair in parts.windows(2) {
        assert!(pair[0].1 <= pair[1].0, "{parts:?}");
    }
    assert!(
        start <= parts[0].0 && parts[2].1 <= end,
        "{parts:?} in {start:?}..{end:?}"
    );
}

/// Where each reference of `report`'s run led: the record that makes it,
/// the path it reached and how many pointers it followed.
fn led(references: &[Referenced]) -> Vec<(String, Option<(String, usize)>)> {
    let mut led: Vec<_> = references
        .iter()
        .map(|referenced| {
            let location = referenced.reached.location.as_ref();
            let reached = location.map(|location| (location.path.clone(), location.pointers.len()));
            (referenced.path.clone(), reached)
        })
        .collect();
    led.sort();
    led
}

#[test]
fn pointer_bytes_at_a_json_path_and_behind_a_symlink_are_followed_and_at_a_schema_path_or_in_a_plain_record_are_not_read()
 {
    // Three pointers to the project, each its own blob. The one at a
    // schema path is a schema, so a reference to it ends there.
    let to_p = |pad: usize| {
        let pad = " ".repeat(pad);
        format!(r#"{{"$type": "board/mount/v1", "path": "/projects/p"}}{pad}"#)
    };
    let scenario = board();
    let see = |target: &str| task(&format!(r#", "see": "{target}""#));
    scenario
        .write(".board/types/lr/ln/v1.schema.json", Item::File(&to_p(0)))
        .write("links/l.json", Item::File(&to_p(1)))
        .write("raw.txt", Item::File(&to_p(2)))
        .write("x.json", Item::Symlink("raw.txt"))
        .write(
            "tasks/a.json",
            Item::File(&see("/.board/types/lr/ln/v1.schema")),
        )
        .write("tasks/b.json", Item::File(&see("/links/l")))
        .write("tasks/c.json", Item::File(&see("/x")))
        .write("tasks/d.json", Item::File(&see("/projects/p")));
    let board = open(&scenario);
    let cache = tempfile::tempdir().unwrap();
    let root = board.repository().path().to_path_buf();
    for _cold_then_warm in 0..2 {
        let index = Index::open(Some(cache.path()), &root).unwrap();
        let snapshot = board.snapshot(index, None, Need::Report).unwrap();
        let through = |task: &str| {
            (
                format!("tasks/{task}.json"),
                Some(("projects/p.json".to_owned(), 1)),
            )
        };
        let direct = (
            "tasks/d.json".to_owned(),
            Some(("projects/p.json".to_owned(), 0)),
        );
        let schema = (
            "tasks/a.json".to_owned(),
            Some((".board/types/lr/ln/v1.schema.json".to_owned(), 0)),
        );
        assert_eq!(
            led(&snapshot.references),
            [schema, through("b"), through("c"), direct]
        );
        // Each pointer once, and neither the schema nor the project its
        // facts call plain.
        assert_eq!(snapshot.report.work.pointers_read, 2);
    }
}

#[test]
fn a_check_lists_each_directory_once_and_a_scoped_one_lists_the_directory_holding_its_scope_once_more()
 {
    let scenario = board();
    let see = |target: &str| task(&format!(r#", "see": "{target}""#));
    scenario
        .write("tasks/a.json", Item::File(&see("/projects/p")))
        .write("tasks/b.json", Item::File(&see("/tasks/a")))
        .write(
            "notes/n.md",
            Item::File("[a](/tasks/a) [p](../projects/p.json)"),
        );
    let board = open(&scenario);
    let cache = tempfile::tempdir().unwrap();
    let listed = |scope: Option<&str>| {
        let scope = scope.map(|scope| scope.parse().unwrap());
        let before = board.repository().directories_listed();
        board.check(scope.as_ref(), Some(cache.path())).unwrap();
        board.repository().directories_listed() - before
    };
    // The root, `.board`, its `types`, `lr`, `task` and `project`, then
    // `projects`, `tasks` and `notes`, cold and warm.
    assert_eq!([listed(None), listed(None)], [9, 9]);
    // Resolving the scope before the snapshot lists the root once more.
    assert_eq!(listed(Some("/tasks")), 9 + 1);
}

#[test]
fn a_file_added_between_warm_runs_is_resolved_by_the_second() {
    let scenario = board();
    let see = |target: &str| task(&format!(r#", "see": "{target}""#));
    scenario.write("tasks/a.json", Item::File(&see("/projects/new")));
    let cache = tempfile::tempdir().unwrap();
    let root = open(&scenario).repository().path().to_path_buf();
    let warm = || Index::open(Some(cache.path()), &root).unwrap();
    let led = |scenario: &Scenario| {
        agrees(scenario, &warm).unwrap();
        led(&filled(&open(scenario), warm()).references)
    };
    assert_eq!(led(&scenario), [("tasks/a.json".to_owned(), None)]);
    scenario.write(
        "projects/new.json",
        Item::File(r#"{"$type": "lr/project/v1"}"#),
    );
    let reached = Some(("projects/new.json".to_owned(), 0));
    assert_eq!(led(&scenario), [("tasks/a.json".to_owned(), reached)]);
}
