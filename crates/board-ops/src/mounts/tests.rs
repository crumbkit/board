//! Checking boards reachable through mounts.

use board_model::{Change, Code, ObjectId, Source};
use board_scenario::{Item, Scenario};
use board_tree::Status;

use crate::tests::{PROJECT_SCHEMA, TASK_SCHEMA, board, open, task};
use crate::{CheckReport, Checked};

fn mount(repo: &str, commit: ObjectId) -> String {
    format!(
        r#"{{"$type": "board/mount/v1", "repo": "{repo}", "follow": "main", "commit": "{commit}"}}"#
    )
}

/// A library board, committed: one valid task, one whose reference dangles
/// in its own board, and its own copy of the types.
fn library() -> (Scenario, ObjectId) {
    let library = Scenario::new();
    let commit = library.commit(&[
        (".board/types/lr/task/v1.schema.json", TASK_SCHEMA),
        (".board/types/lr/project/v1.schema.json", PROJECT_SCHEMA),
        ("projects/p.json", r#"{"$type": "lr/project/v1"}"#),
        ("tasks/ok.json", &task(r#", "project": "/projects/p""#)),
        ("tasks/dangling.json", &task(r#", "see": "/nowhere""#)),
    ]);
    (library, commit)
}

fn mounted(report: &CheckReport) -> Vec<(String, Status)> {
    report
        .entries
        .iter()
        .filter(|entry| matches!(entry.source, Source::Commit(_)))
        .map(|entry| (entry.logical.as_ref().unwrap().to_string(), entry.status))
        .collect()
}

fn check(scenario: &Scenario, scope: Option<&str>) -> CheckReport {
    let scope = scope.map(|scope| scope.parse().unwrap());
    open(scenario).check(scope.as_ref(), None).unwrap()
}

#[test]
fn a_mounted_board_is_checked_in_itself_and_named_under_its_mount_point() {
    let (library, commit) = library();
    let scenario = board();
    scenario.write(
        "vendor/lib.json",
        Item::File(&mount(&library.path(), commit)),
    );
    let report = check(&scenario, None);
    assert_eq!(
        mounted(&report),
        [
            (
                "/vendor/lib/.board/types/lr/project/v1.schema".to_owned(),
                Status::Ok
            ),
            (
                "/vendor/lib/.board/types/lr/task/v1.schema".to_owned(),
                Status::Ok
            ),
            ("/vendor/lib/projects/p".to_owned(), Status::Ok),
            (
                "/vendor/lib/tasks/dangling".to_owned(),
                Status::Degraded(Code::RefDangling)
            ),
            ("/vendor/lib/tasks/ok".to_owned(), Status::Ok),
        ]
    );
    let ok = report
        .entries
        .iter()
        .find(|entry| entry.path == "tasks/ok.json")
        .unwrap();
    assert_eq!(
        (&ok.source, ok.change),
        (&Source::Commit(commit), Change::Committed)
    );
}

#[test]
fn a_board_mounted_twice_is_indexed_once_under_its_first_route_and_in_scope_on_either() {
    let (library, commit) = library();
    let scenario = board();
    let to_library = mount(&library.path(), commit);
    scenario
        .write("b.json", Item::File(&to_library))
        .write("a.json", Item::File(&to_library));
    let report = check(&scenario, None);
    let entries = mounted(&report);
    assert_eq!(entries.len(), 5, "one row per entry, not per mount point");
    assert!(
        entries
            .iter()
            .all(|(logical, _)| logical.starts_with("/a/"))
    );
    assert_eq!(mounted(&check(&scenario, Some("/b"))), entries);
    assert_eq!(mounted(&check(&scenario, Some("/b/tasks"))).len(), 2);
    assert_eq!(mounted(&check(&scenario, Some("/a/tasks"))).len(), 2);
    assert_eq!(mounted(&check(&scenario, Some("/projects"))), []);
}

#[test]
fn a_mount_inside_a_mounted_board_is_followed_and_reads_against_its_own_repository() {
    let (library, commit) = library();
    let middle = Scenario::new();
    let middle_commit = middle.commit(&[("deep.json", &mount(&library.path(), commit))]);
    let scenario = board();
    scenario.write("m.json", Item::File(&mount(&middle.path(), middle_commit)));
    let entries = mounted(&check(&scenario, None));
    assert!(entries.contains(&("/m/deep".to_owned(), Status::Ok)));
    assert!(entries.contains(&("/m/deep/tasks/ok".to_owned(), Status::Ok)));
}

#[test]
fn a_mount_that_cannot_be_read_degrades_only_its_pointer() {
    let (library, commit) = library();
    let scenario = board();
    scenario
        .write("good.json", Item::File(&mount(&library.path(), commit)))
        .write(
            "gone.json",
            Item::File(&mount("/no/such/repository", commit)),
        );
    let report = check(&scenario, None);
    let status = |path: &str| -> Status {
        report
            .entries
            .iter()
            .find(|entry: &&Checked| entry.source == Source::WorkingTree && entry.path == path)
            .unwrap()
            .status
    };
    assert_eq!(
        status("gone.json"),
        Status::Degraded(Code::MountRepoMissing)
    );
    assert_eq!(status("good.json"), Status::Ok);
    assert_eq!(mounted(&report).len(), 5);
}

#[test]
fn a_mounted_commit_is_listed_once_and_not_again_when_warm() {
    let (library, commit) = library();
    let scenario = board();
    let to_library = mount(&library.path(), commit);
    scenario
        .write("a.json", Item::File(&to_library))
        .write("b.json", Item::File(&to_library));
    let once = board();
    once.write("a.json", Item::File(&to_library));
    let cache = tempfile::tempdir().unwrap();
    let board = open(&scenario);
    let cold = board.check(None, Some(cache.path())).unwrap();
    // Eight trees in the library; the root board has no commit.
    assert_eq!(cold.work.trees, 8);
    assert_eq!(open(&once).check(None, None).unwrap().work.trees, 8);
    let warm = board.check(None, Some(cache.path())).unwrap();
    assert_eq!(
        (warm.work.trees, warm.work.parsed, warm.work.validated),
        (0, 0, 0)
    );
    assert_eq!(warm.entries, cold.entries);
}

#[test]
fn a_path_too_long_under_its_mount_point_is_malformed_and_its_shorter_twin_is_ok() {
    // Nineteen segments of 204 bytes and a last one of `last` bytes: 4091
    // bytes as a logical path in the library when `last` is 195, which fits,
    // and 4102 under /vendor/lib, which does not. One of 180 fits both.
    let path = |last: usize| {
        let mut segments = vec!["a".repeat(204); 19];
        segments.push("b".repeat(last));
        format!("{}.json", segments.join("/"))
    };
    let (long, short) = (path(195), path(180));
    let library = Scenario::new();
    let commit = library.commit(&[
        (".board/types/lr/project/v1.schema.json", PROJECT_SCHEMA),
        (&long, r#"{"$type": "lr/project/v1"}"#),
        (&short, r#"{"$type": "lr/project/v1"}"#),
    ]);
    let scenario = board();
    scenario.write(
        "vendor/lib.json",
        Item::File(&mount(&library.path(), commit)),
    );
    let report = check(&scenario, None);
    let status = |path: &str| -> Status {
        report
            .entries
            .iter()
            .find(|entry: &&Checked| entry.path == path)
            .unwrap()
            .status
    };
    assert_eq!(status(&long), Status::Degraded(Code::PathMalformed));
    assert_eq!(status(&short), Status::Ok);
}
