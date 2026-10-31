//! Scoped runs walk only the mounted sources their scope reaches, and answer
//! warm as cold.

use std::path::Path;

use board_model::{LogicalPath, ObjectId, Source};
use board_scenario::{Item, Scenario};

use crate::snapshot::Need;
use crate::tests::{PROJECT_SCHEMA, TASK_SCHEMA, board, open, task};
use crate::{Checked, LsReport};

fn mount(repo: &str, commit: ObjectId) -> String {
    format!(
        r#"{{"$type": "board/mount/v1", "repo": "{repo}", "follow": "main", "commit": "{commit}"}}"#
    )
}

/// A library board holding the types, a project, a valid task, a task whose
/// reference dangles, and `files`, committed.
fn library(files: &[(&str, &str)]) -> (Scenario, ObjectId) {
    let library = Scenario::new();
    let ok = task(r#", "project": "/projects/p""#);
    let dangling = task(r#", "see": "/nowhere""#);
    let mut all = vec![
        (".board/types/lr/task/v1.schema.json", TASK_SCHEMA),
        (".board/types/lr/project/v1.schema.json", PROJECT_SCHEMA),
        ("projects/p.json", r#"{"$type": "lr/project/v1"}"#),
        ("tasks/ok.json", ok.as_str()),
        ("tasks/dangling.json", dangling.as_str()),
    ];
    all.extend_from_slice(files);
    let commit = library.commit(&all);
    (library, commit)
}

/// A board that mounts `a` at `/repos/a`, which mounts `inner` at `/i`, and
/// `b` at both `/repos/b` and `/z/b`, whose first route is `/z/b`. Each
/// library is returned with its name and commit, so it outlives the board's
/// runs and a test can name the sources a run walked.
fn mounting() -> (Scenario, [Library; 3]) {
    let (inner, i) = library(&[]);
    let (a, a_commit) = library(&[("i.json", &mount(&inner.path(), i))]);
    let (b, b_commit) = library(&[("notes.md", "b")]);
    let scenario = board();
    scenario
        .write("repos/a.json", Item::File(&mount(&a.path(), a_commit)))
        .write("repos/b.json", Item::File(&mount(&b.path(), b_commit)))
        .write("z/b.json", Item::File(&mount(&b.path(), b_commit)))
        .write(
            "tasks/t.json",
            Item::File(&task(r#", "see": "/repos/a/i/projects/p""#)),
        );
    (
        scenario,
        [("a", a, a_commit), ("b", b, b_commit), ("inner", inner, i)],
    )
}

/// A mounted library's name, its repository and its commit.
type Library = (&'static str, Scenario, ObjectId);

fn scope(path: &str) -> Option<LogicalPath> {
    (path != "/").then(|| path.parse().unwrap())
}

/// The entries a check scoped to `path` reports, through the index in
/// `cache` or a cold one.
fn checked(scenario: &Scenario, path: &str, cache: Option<&Path>) -> Vec<Checked> {
    let report = open(scenario).check(scope(path).as_ref(), cache).unwrap();
    report.entries
}

/// The names of the mounted sources, sorted, that `ls` of `path` walked
/// through the index in `cache`. It decides every source its scope reaches.
fn ls_walked(
    (scenario, libraries): (&Scenario, &[Library]),
    path: &str,
    cache: &Path,
) -> Vec<&'static str> {
    let board = open(scenario);
    let index = board.index(Some(cache)).unwrap();
    let snapshot = board.snapshot(index, scope(path).as_ref(), Need::Report);
    let snapshot = snapshot.unwrap();
    let mut walked: Vec<&str> = (snapshot.walked.iter())
        .filter_map(|(source, _)| match source {
            Source::Commit(commit) => libraries.iter().find(|l| l.2 == *commit),
            Source::WorkingTree => None,
        })
        .map(|(name, _, _)| *name)
        .collect();
    walked.sort_unstable();
    assert_eq!(snapshot.report.work.walked, walked.len(), "{path}");
    walked
}

/// What `ls` of `path` lists, without the work it did or where its index
/// was.
fn listed(scenario: &Scenario, path: &str, cache: Option<&Path>) -> LsReport {
    let report = open(scenario).ls(&path.parse().unwrap(), cache).unwrap();
    LsReport {
        work: crate::Refreshed::default(),
        memory: None,
        ..report
    }
}

/// Every scope the tests try, and the mounted sources it reaches: a
/// working-tree directory, a mount, a directory inside a mount, a directory
/// that holds mounts, a mount two levels deep, and the second route to a
/// commit mounted twice. The scope `/repos/a/i/projects` reaches `a`
/// because its route crosses `a`'s mount at `/i`: a directory `i/` in `a`
/// would sit in scope, shadowed by that mount.
const SCOPES: [(&str, &[&str]); 6] = [
    ("/tasks", &[]),
    ("/repos/a", &["a", "inner"]),
    ("/repos/a/tasks", &["a"]),
    ("/repos", &["a", "b", "inner"]),
    ("/repos/a/i/projects", &["a", "inner"]),
    ("/repos/b", &["b"]),
];

#[test]
fn a_warm_scoped_ls_walks_only_the_mounts_its_scope_reaches_and_its_wider_twin_walks_each() {
    let (scenario, libraries) = mounting();
    let board = (&scenario, libraries.as_slice());
    let cache = tempfile::tempdir().unwrap();
    let walked = |path| ls_walked(board, path, cache.path());
    let every = ["a", "b", "inner"];
    assert_eq!(walked("/"), every, "cold: every source is walked");
    for (path, reached) in SCOPES {
        assert_eq!(walked(path), reached, "{path}");
        assert_eq!(walked("/"), every, "after {path}");
    }
    let report = open(&scenario).check(scope("/tasks").as_ref(), Some(cache.path()));
    assert_eq!(
        report.unwrap().work.kept,
        0,
        "a check reads no source as kept"
    );
}

/// The mounted sources a check of `path` through the index in `cache` read
/// from kept rows, walked, and the blobs it parsed.
fn check_work(scenario: &Scenario, path: &str, cache: &Path) -> (usize, usize, usize) {
    let work = open(scenario)
        .check(scope(path).as_ref(), Some(cache))
        .unwrap()
        .work;
    (work.kept, work.walked, work.parsed)
}

#[test]
fn a_warm_check_reads_each_kept_source_and_after_one_moves_walks_only_that_one() {
    let (scenario, [_a, (_, b, _), _inner]) = mounting();
    let cache = tempfile::tempdir().unwrap();
    let work = |path| check_work(&scenario, path, cache.path());
    assert_eq!(work("/").0, 0, "cold");
    assert_eq!(work("/"), (3, 0, 0));
    let moved = b.commit(&[("notes.md", "moved")]);
    let pointer = mount(&b.path(), moved);
    scenario
        .write("repos/b.json", Item::File(&pointer))
        .write("z/b.json", Item::File(&pointer));
    let (kept, walked, _) = work("/");
    assert_eq!(
        (kept, walked),
        (2, 1),
        "a and the board it mounts are still kept"
    );
    assert_eq!(work("/"), (3, 0, 0));
}

#[test]
fn a_warm_check_of_a_mount_reads_its_kept_rows_and_one_of_a_directory_inside_it_walks_it() {
    let (scenario, _libraries) = mounting();
    let cache = tempfile::tempdir().unwrap();
    let work = |path| check_work(&scenario, path, cache.path());
    work("/");
    assert_eq!(work("/repos/a"), (2, 0, 0));
    assert_eq!(work("/repos/a/tasks"), (0, 1, 0));
}

#[test]
fn a_warm_scoped_check_and_ls_answer_as_cold_ones() {
    let (scenario, _libraries) = mounting();
    let cache = tempfile::tempdir().unwrap();
    checked(&scenario, "/", Some(cache.path()));
    for (path, _) in SCOPES {
        let warm = checked(&scenario, path, Some(cache.path()));
        let cold = checked(&scenario, path, None);
        assert!(!cold.is_empty(), "{path}");
        assert_eq!(warm, cold, "check {path}");
        if path == "/tasks" {
            // Its reference resolves into a source the run does not walk.
            assert_eq!(warm[0].path, "tasks/t.json");
            assert_eq!(warm[0].status, board_tree::Status::Ok);
        }
        let warm = listed(&scenario, path, Some(cache.path()));
        assert_eq!(warm, listed(&scenario, path, None), "ls {path}");
    }
}

#[test]
fn a_scope_at_the_second_route_to_a_commit_reports_its_entries_under_the_first() {
    let (scenario, _libraries) = mounting();
    let entries = checked(&scenario, "/repos/b", None);
    let logical: Vec<String> = entries
        .iter()
        .filter(|entry| entry.source != Source::WorkingTree)
        .filter_map(|entry| Some(entry.logical.as_ref()?.to_string()))
        .collect();
    assert_eq!(logical.len(), 6);
    assert!(
        logical.iter().all(|path| path.starts_with("/z/b/")),
        "{logical:?}"
    );
}

#[test]
fn an_unscoped_check_after_cold_and_warm_scoped_ones_answers_as_a_cold_one() {
    let (scenario, libraries) = mounting();
    let board = (&scenario, libraries.as_slice());
    let cold = checked(&scenario, "/", None);
    for (path, reached) in SCOPES {
        let cache = tempfile::tempdir().unwrap();
        checked(&scenario, path, Some(cache.path()));
        checked(&scenario, path, Some(cache.path()));
        let walked = ls_walked(board, path, cache.path());
        assert_eq!(walked, reached, "{path}");
        let warm = checked(&scenario, "/", Some(cache.path()));
        assert_eq!(warm, cold, "after {path}");
    }
}
