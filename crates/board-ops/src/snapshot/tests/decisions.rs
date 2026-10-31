//! Decisions the index keeps for mounted sources. A warm query reads them
//! and answers as a cold one after each thing a decision depends on
//! changes, and decides again only the sources that change touched.

use std::path::Path;

use board_index::Index;
use board_model::ObjectId;
use board_scenario::{Item, Scenario};

use super::{EVERY_TABLE, filled};
use crate::Refreshed;
use crate::tests::{Library, PROJECT_SCHEMA, TASK_SCHEMA, board, open, task};

fn mount(repo: &str, commit: ObjectId) -> String {
    format!(
        r#"{{"$type": "board/mount/v1", "repo": "{repo}", "follow": "main", "commit": "{commit}"}}"#
    )
}

/// The build these tests decide as, unless one says otherwise.
const BUILD: &str = "test";

/// Queries `scenario` as `build` through the index in `cache` and through a
/// cold one, requires every table and an unordered, bounded query to
/// answer alike, then requires a check to report alike through both, and
/// returns the warm query's work.
fn query_as(scenario: &Scenario, cache: &Path, build: &'static str) -> Refreshed {
    let mut board = open(scenario);
    board.built_by(build);
    let root = board.repository().path().to_path_buf();
    let warm = filled(&board, Index::open(Some(cache), &root).unwrap());
    let cold = filled(&board, Index::open(None, &root).unwrap());
    let unordered = "SELECT source, path, code FROM entries LIMIT 7";
    for sql in EVERY_TABLE.iter().chain([&unordered]) {
        let [warm, cold] = [&warm, &cold].map(|filled| filled.view.query(sql, 10_000, None));
        assert_eq!(warm.unwrap(), cold.unwrap(), "{sql}");
    }
    let [warm_check, cold_check] = [Some(cache), None].map(|cache| board.check(None, cache));
    assert_eq!(warm_check.unwrap().entries, cold_check.unwrap().entries);
    warm.report.work
}

/// How many mounted sources a warm query kept, after answering as a cold
/// one.
fn kept(scenario: &Scenario, cache: &Path) -> usize {
    query_as(scenario, cache, BUILD).kept
}

/// A library board holding the types, a project and `files`, committed.
fn library(files: &[(&str, &str)]) -> (Scenario, ObjectId) {
    let library = Scenario::new();
    let mut all = vec![
        (".board/types/lr/task/v1.schema.json", TASK_SCHEMA),
        (".board/types/lr/project/v1.schema.json", PROJECT_SCHEMA),
        ("projects/p.json", r#"{"$type": "lr/project/v1"}"#),
    ];
    all.extend_from_slice(files);
    let commit = library.commit(&all);
    (library, commit)
}

/// The loose object `id` in `scenario`'s repository, as a path in it.
fn object(id: ObjectId) -> String {
    let hex = id.to_string();
    format!(".git/objects/{}/{}", &hex[..2], &hex[2..])
}

#[test]
fn a_warm_query_keeps_every_mounted_source_and_after_one_moves_decides_only_that_one() {
    let scenario = board();
    let vendor = Library::mounted(&scenario);
    let (other, commit) = library(&[]);
    scenario.write("other.json", Item::File(&mount(&other.path(), commit)));
    let cache = tempfile::tempdir().unwrap();
    assert_eq!(
        kept(&scenario, cache.path()),
        0,
        "a cold index keeps nothing"
    );
    let warm = query_as(&scenario, cache.path(), BUILD);
    assert_eq!((warm.kept, warm.trees), (2, 0));
    // The move also turns the library's plain project into a link.
    vendor.moved(&scenario);
    assert_eq!(kept(&scenario, cache.path()), 1, "the other is still kept");
    assert_eq!(kept(&scenario, cache.path()), 2);
}

#[test]
fn a_mount_moved_to_another_commit_with_the_same_tree_is_kept_and_named_by_the_new_commit() {
    let (library, first) = library(&[("tasks/t.json", &task(r#", "see": "/projects/p""#))]);
    let second = library.commit(&[
        (".board/types/lr/task/v1.schema.json", TASK_SCHEMA),
        (".board/types/lr/project/v1.schema.json", PROJECT_SCHEMA),
        ("projects/p.json", r#"{"$type": "lr/project/v1"}"#),
        ("tasks/t.json", &task(r#", "see": "/projects/p""#)),
    ]);
    assert_ne!(first, second);
    let scenario = board();
    scenario.write("vendor.json", Item::File(&mount(&library.path(), first)));
    let cache = tempfile::tempdir().unwrap();
    kept(&scenario, cache.path());
    scenario.write("vendor.json", Item::File(&mount(&library.path(), second)));
    assert_eq!(kept(&scenario, cache.path()), 1);
    let board = open(&scenario);
    let index = Index::open(Some(cache.path()), board.repository().path()).unwrap();
    let sources = "SELECT DISTINCT source FROM entries WHERE source <> 'worktree'";
    let rows = filled(&board, index)
        .view
        .query(sources, 10, None)
        .unwrap()
        .rows;
    assert_eq!(rows, [[serde_json::Value::from(second.to_string())]]);
}

#[test]
fn a_mount_moved_to_a_longer_route_is_decided_again_and_a_path_too_long_beneath_it_is_malformed() {
    // Nineteen segments of 204 bytes and one of 195: 4091 bytes as a
    // logical path in the library, which fits under /v and not under
    // /vendor/lib.
    let mut segments = vec!["a".repeat(204); 19];
    segments.push("b".repeat(195));
    let long = format!("{}.json", segments.join("/"));
    let (library, commit) = library(&[(&long, r#"{"$type": "lr/project/v1"}"#)]);
    let scenario = board();
    let pointer = mount(&library.path(), commit);
    scenario.write("v.json", Item::File(&pointer));
    let cache = tempfile::tempdir().unwrap();
    kept(&scenario, cache.path());
    assert_eq!(kept(&scenario, cache.path()), 1);
    scenario
        .write("v.json", Item::Deleted)
        .write("vendor/lib.json", Item::File(&pointer));
    assert_eq!(kept(&scenario, cache.path()), 0, "the route is in the key");
    let board = open(&scenario);
    let index = Index::open(Some(cache.path()), board.repository().path()).unwrap();
    let sql = format!("SELECT code FROM entries WHERE path = '{long}'");
    let rows = filled(&board, index)
        .view
        .query(&sql, 1, None)
        .unwrap()
        .rows;
    assert_eq!(rows, [["board::path::malformed"]]);
}

/// A board that mounts `middle` at `/m`, which mounts `inner` at `/i`,
/// which mounts `innermost` at `/j`, each by its absolute path.
fn three_deep() -> (Scenario, [Scenario; 3]) {
    let (innermost, j) = library(&[]);
    let (inner, i) = library(&[("j.json", &mount(&innermost.path(), j))]);
    let (middle, m) = library(&[
        ("i.json", &mount(&inner.path(), i)),
        ("tasks/t.json", &task(r#", "see": "/i/j/projects/p""#)),
    ]);
    let scenario = board();
    scenario.write("m.json", Item::File(&mount(&middle.path(), m)));
    (scenario, [middle, inner, innermost])
}

#[test]
fn a_repository_deleted_two_mounts_down_decides_both_mounts_above_it_again_and_its_twin_keeps_all()
{
    let (scenario, [_middle, _inner, innermost]) = three_deep();
    let cache = tempfile::tempdir().unwrap();
    kept(&scenario, cache.path());
    assert_eq!(kept(&scenario, cache.path()), 3);
    innermost.write(".git", Item::Deleted);
    assert_eq!(
        kept(&scenario, cache.path()),
        0,
        "a probe changed below each"
    );
    assert_eq!(kept(&scenario, cache.path()), 2);
}

#[test]
fn a_mounted_commit_its_repository_lost_decides_the_mounts_above_it_again_as_does_losing_the_repository()
 {
    let (scenario, [_middle, _inner, innermost]) = three_deep();
    let cache = tempfile::tempdir().unwrap();
    kept(&scenario, cache.path());
    assert_eq!(kept(&scenario, cache.path()), 3);
    let j = innermost.head().unwrap().id;
    innermost.write(&object(j), Item::Deleted);
    assert_eq!(
        kept(&scenario, cache.path()),
        0,
        "a probe changed below each"
    );
    assert_eq!(kept(&scenario, cache.path()), 2);
    // Only the refusal's code changes now.
    innermost.write(".git", Item::Deleted);
    assert_eq!(kept(&scenario, cache.path()), 0);
}

#[test]
fn a_relative_nested_mount_read_from_a_moved_repository_is_decided_again() {
    let (inner, i) = library(&[]);
    let name = inner.workdir().file_name().unwrap().to_str().unwrap();
    let files = [("i.json", mount(&format!("../{name}"), i))];
    let files: Vec<(&str, &str)> = files.iter().map(|(p, t)| (*p, t.as_str())).collect();
    let (middle, m) = library(&files);
    // The same tree in a repository one directory deeper, where `../`
    // names no repository.
    let holder = Scenario::new();
    let moved = Scenario::within(&holder);
    let mut all = vec![
        (".board/types/lr/task/v1.schema.json", TASK_SCHEMA),
        (".board/types/lr/project/v1.schema.json", PROJECT_SCHEMA),
        ("projects/p.json", r#"{"$type": "lr/project/v1"}"#),
    ];
    all.extend_from_slice(&files);
    let moved_commit = moved.commit(&all);
    let scenario = board();
    scenario.write("m.json", Item::File(&mount(&middle.path(), m)));
    let cache = tempfile::tempdir().unwrap();
    kept(&scenario, cache.path());
    assert_eq!(kept(&scenario, cache.path()), 2);
    scenario.write("m.json", Item::File(&mount(&moved.path(), moved_commit)));
    assert_eq!(kept(&scenario, cache.path()), 0);
    assert_eq!(kept(&scenario, cache.path()), 1);
}

#[test]
fn a_mounted_source_with_a_file_that_cannot_be_read_is_never_kept_and_its_twin_is() {
    for (missing, expected) in [(true, 0), (false, 1)] {
        let library = Scenario::new();
        let mut items: Vec<(&[u8], Item<'_>)> = vec![(b"p.json", Item::File("{}"))];
        if missing {
            items.push((b"q.json", Item::MissingBlob));
        }
        let commit = library.commit_items(&items);
        let scenario = board();
        scenario.write("vendor.json", Item::File(&mount(&library.path(), commit)));
        let cache = tempfile::tempdir().unwrap();
        kept(&scenario, cache.path());
        assert_eq!(
            kept(&scenario, cache.path()),
            expected,
            "missing: {missing}"
        );
    }
}

#[test]
fn a_source_whose_reference_reaches_a_blob_that_cannot_be_read_is_never_kept_and_its_twin_is() {
    for (missing, expected) in [(true, 0), (false, 2)] {
        let inner = Scenario::new();
        let p = if missing {
            Item::MissingBlob
        } else {
            Item::File(r#"{"$type": "lr/project/v1"}"#)
        };
        let i = inner.commit_items(&[(b"p.json", p)]);
        let (middle, m) = library(&[
            ("i.json", &mount(&inner.path(), i)),
            ("tasks/t.json", &task(r#", "project": "/i/p""#)),
        ]);
        let scenario = board();
        scenario.write("m.json", Item::File(&mount(&middle.path(), m)));
        let cache = tempfile::tempdir().unwrap();
        kept(&scenario, cache.path());
        assert_eq!(
            kept(&scenario, cache.path()),
            expected,
            "missing: {missing}"
        );
    }
}

#[test]
fn a_scoped_check_keeps_no_decision_and_its_unscoped_twin_keeps_each() {
    for (scope, expected) in [(Some("/vendor/tasks"), 0), (None, 1)] {
        let scenario = board();
        let _vendor = Library::mounted(&scenario);
        let cache = tempfile::tempdir().unwrap();
        let scope = scope.map(|scope| scope.parse().unwrap());
        open(&scenario)
            .check(scope.as_ref(), Some(cache.path()))
            .unwrap();
        assert_eq!(kept(&scenario, cache.path()), expected, "{scope:?}");
    }
}

#[test]
fn another_build_lists_pointers_and_decides_again_and_the_same_build_keeps_both() {
    for (second, expected, lists) in [("b", 0, 2), ("a", 1, 1)] {
        let scenario = board();
        let _vendor = Library::mounted(&scenario);
        let cache = tempfile::tempdir().unwrap();
        query_as(&scenario, cache.path(), "a");
        assert_eq!(query_as(&scenario, cache.path(), second).kept, expected);
        let mut board = open(&scenario);
        board.built_by(second);
        let index = Index::open(Some(cache.path()), board.repository().path()).unwrap();
        let sql = "SELECT count(DISTINCT scan) FROM main.pointers";
        let rows = filled(&board, index).view.query(sql, 1, None).unwrap().rows;
        assert_eq!(rows, [[lists]], "{second}");
    }
}

#[test]
fn a_source_that_mounts_a_commit_through_another_repository_than_its_walk_is_never_kept() {
    for (through_clone, expected) in [(true, 1), (false, 2)] {
        let (library, commit) = library(&[]);
        let clone = Scenario::clone_of(&library);
        let repo = if through_clone {
            clone.path()
        } else {
            library.path()
        };
        let (middle, m) = self::library(&[("l.json", &mount(&repo, commit))]);
        let scenario = board();
        scenario
            .write("a.json", Item::File(&mount(&library.path(), commit)))
            .write("m.json", Item::File(&mount(&middle.path(), m)));
        let cache = tempfile::tempdir().unwrap();
        kept(&scenario, cache.path());
        assert_eq!(kept(&scenario, cache.path()), expected, "{through_clone}");
    }
}

#[test]
fn a_source_holding_a_mount_whose_route_is_too_long_is_never_kept_and_its_shorter_twin_is() {
    // The pointer's logical path is 4091 bytes in the library when `last`
    // is 195, and 4102 under /vendor/lib; with 170 it fits there.
    for (last, expected) in [(195, 0), (170, 2)] {
        let mut segments = vec!["a".repeat(204); 19];
        segments.push("b".repeat(last));
        let at = format!("{}.json", segments.join("/"));
        let (inner, i) = library(&[]);
        let (library, commit) = library(&[(&at, &mount(&inner.path(), i))]);
        let scenario = board();
        scenario.write(
            "vendor/lib.json",
            Item::File(&mount(&library.path(), commit)),
        );
        let cache = tempfile::tempdir().unwrap();
        kept(&scenario, cache.path());
        assert_eq!(kept(&scenario, cache.path()), expected, "{last}");
    }
}

/// A board mounting a library whose note links through a pointer at
/// `pointer`, into a board mounted there. Returns the board, the library
/// and the board the link reaches.
fn linked_through(pointer: &str) -> (Scenario, Scenario, Scenario) {
    let (target, t) = library(&[]);
    let through = pointer.strip_suffix(".json").unwrap();
    let note = format!("[p](/{through}/projects/p)");
    let (library, l) = library(&[(pointer, &mount(&target.path(), t)), ("notes/n.md", &note)]);
    let scenario = board();
    scenario.write("l.json", Item::File(&mount(&library.path(), l)));
    (scenario, library, target)
}

/// The code of `notes/n.md` in `scenario`, queried warm through `cache`
/// after answering as a cold query.
fn note_code(scenario: &Scenario, cache: &Path) -> Vec<Vec<serde_json::Value>> {
    query_as(scenario, cache, BUILD);
    let board = open(scenario);
    let index = Index::open(Some(cache), board.repository().path()).unwrap();
    let sql = "SELECT code FROM entries WHERE path = 'notes/n.md'";
    filled(&board, index).view.query(sql, 1, None).unwrap().rows
}

#[test]
fn a_source_linking_through_a_pointer_in_a_schema_file_is_kept_alone_and_its_twin_in_a_record_file_with_its_mount()
 {
    for (pointer, expected) in [
        (".board/types/acme/thing/v1.schema.json", 1),
        ("things/t.json", 2),
    ] {
        let (scenario, _library, _target) = linked_through(pointer);
        let cache = tempfile::tempdir().unwrap();
        kept(&scenario, cache.path());
        assert_eq!(kept(&scenario, cache.path()), expected, "{pointer}");
    }
}

#[test]
fn a_link_through_a_pointer_in_a_schema_file_dangles_warm_as_cold_and_its_twin_in_a_record_file_resolves()
 {
    for (pointer, code) in [
        (
            ".board/types/acme/thing/v1.schema.json",
            serde_json::json!("board::ref::dangling"),
        ),
        ("things/t.json", serde_json::Value::Null),
    ] {
        let (scenario, _library, _target) = linked_through(pointer);
        let cache = tempfile::tempdir().unwrap();
        assert_eq!(
            note_code(&scenario, cache.path()),
            [[code.clone()]],
            "{pointer}"
        );
        assert_eq!(note_code(&scenario, cache.path()), [[code]], "{pointer}");
    }
}

#[test]
fn a_source_crossing_a_mount_its_walk_does_not_list_is_never_kept_and_dangles_warm_as_cold_and_its_listed_twin_is_kept()
 {
    // `M.json` beside `m.json` degrades both with a case collision, so the
    // walk lists no pointer at `m.json`, though resolution follows it.
    for (colliding, expected) in [(true, 0), (false, 2)] {
        let (target, t) = library(&[]);
        let to_target = mount(&target.path(), t);
        let mut files = vec![
            ("m.json", to_target.as_str()),
            ("notes/n.md", "[p](/m/projects/p)"),
        ];
        if colliding {
            files.push(("M.json", "{}"));
        }
        let (library, l) = library(&files);
        let scenario = board();
        scenario.write("l.json", Item::File(&mount(&library.path(), l)));
        let cache = tempfile::tempdir().unwrap();
        kept(&scenario, cache.path());
        assert_eq!(kept(&scenario, cache.path()), expected, "{colliding}");
        assert_eq!(
            note_code(&scenario, cache.path()),
            [[serde_json::Value::Null]]
        );
        target.write(".git", Item::Deleted);
        let dangling = serde_json::json!("board::ref::dangling");
        assert_eq!(
            note_code(&scenario, cache.path()),
            [[dangling]],
            "{colliding}"
        );
    }
}

#[test]
fn an_empty_build_keeps_and_stores_nothing_and_a_named_one_keeps_each_source() {
    for (build, expected, stored) in [("", 0, 0), (BUILD, 1, 1)] {
        let scenario = board();
        let _vendor = Library::mounted(&scenario);
        let cache = tempfile::tempdir().unwrap();
        query_as(&scenario, cache.path(), build);
        assert_eq!(query_as(&scenario, cache.path(), build).kept, expected);
        let mut board = open(&scenario);
        board.built_by(build);
        let index = Index::open(Some(cache.path()), board.repository().path()).unwrap();
        let sql = "SELECT (SELECT count(*) FROM main.decided) + (SELECT count(DISTINCT scan) FROM main.pointers) > 0";
        let rows = filled(&board, index).view.query(sql, 1, None).unwrap().rows;
        assert_eq!(rows, [[stored]], "{build:?}");
    }
}

/// A board mounting, at `/lib`, a library whose entries fail in every way
/// a check details, and at `/vendor/lib` a board holding a path too long
/// to name there.
fn every_detail() -> (Scenario, [Scenario; 2]) {
    let mut segments = vec!["a".repeat(204); 19];
    segments.push("b".repeat(195));
    let long = format!("{}.json", segments.join("/"));
    let (long_board, long_commit) = library(&[(&long, r#"{"$type": "lr/project/v1"}"#)]);
    let two = task(r#", "project": "/nowhere", "see": "/gone""#);
    let wrong = task(r#", "project": "/tasks/two""#);
    let into = task(r#", "see": "/i/projects/p""#);
    let gone = mount("/no/such/repository", long_commit);
    let schemas = [
        (".board/types/lr/task/v1.schema.json", TASK_SCHEMA),
        (".board/types/lr/project/v1.schema.json", PROJECT_SCHEMA),
    ];
    let lib = Scenario::new();
    let mut items: Vec<(&[u8], Item<'_>)> = (schemas.iter())
        .map(|(path, text)| (path.as_bytes(), Item::File(text)))
        .collect();
    items.extend([
        (
            &b"projects/p.json"[..],
            Item::File(r#"{"$type": "lr/project/v1"}"#),
        ),
        (b"tasks/two.json", Item::File(&two)),
        (b"tasks/wrong.json", Item::File(&wrong)),
        (b"tasks/into.json", Item::File(&into)),
        (
            b"things/x.json",
            Item::File(r#"{"$type": "lr/missing/v1"}"#),
        ),
        (b"links/dangling.json", Item::Symlink("gone.json")),
        (b"bad.json", Item::File("{")),
        (b"i.json", Item::File(&gone)),
    ]);
    let commit = lib.commit_items(&items);
    let scenario = board();
    scenario
        .write("lib.json", Item::File(&mount(&lib.path(), commit)))
        .write(
            "vendor/lib.json",
            Item::File(&mount(&long_board.path(), long_commit)),
        );
    (scenario, [lib, long_board])
}

#[test]
fn a_warm_check_of_kept_sources_reports_every_detail_as_a_cold_one_scoped_or_not() {
    let (scenario, _libraries) = every_detail();
    let cache = tempfile::tempdir().unwrap();
    let board = open(&scenario);
    let check = |scope: Option<&str>, cache| {
        let scope: Option<board_model::LogicalPath> = scope.map(|scope| scope.parse().unwrap());
        board.check(scope.as_ref(), cache).unwrap()
    };
    check(None, Some(cache.path()));
    for scope in [None, Some("/lib"), Some("/lib/tasks"), Some("/vendor")] {
        let warm = check(scope, Some(cache.path()));
        assert_eq!(warm.entries, check(scope, None).entries, "{scope:?}");
    }
    let warm = check(None, Some(cache.path()));
    assert_eq!(warm.work.kept, 2);
    let mounted =
        (warm.entries.iter()).filter(|entry| entry.source != board_model::Source::WorkingTree);
    let details: Vec<_> = mounted.filter_map(|entry| entry.detail.as_ref()).collect();
    assert!(
        details.iter().any(|detail| detail.failures == 2),
        "several failures"
    );
    assert!(
        details
            .iter()
            .any(|detail| detail.at.is_some() && detail.mount.is_some()),
        "at and mount"
    );
    assert!(
        details.iter().any(|detail| detail.target.is_some()),
        "a symlink's target"
    );
    assert!(
        warm.entries.iter().any(|entry| entry.schema.is_some()),
        "a named schema"
    );
    assert!(
        warm.entries.iter().any(
            |entry| entry.logical.is_none() && entry.source != board_model::Source::WorkingTree
        ),
        "a path too long to name"
    );
}
