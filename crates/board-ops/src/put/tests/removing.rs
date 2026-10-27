//! `board rm` and the removal lines of `put --batch`.

use board_model::{Code, Fields, ObjectId};
use board_scenario::{Item, Scenario};
use board_tree::LOCK_WAIT;

use super::{board, committing, file, path, put};
use crate::put::{Expect, PutError, PutReport, Removing, Unmatched};
use crate::tests::{open, task};

fn rm(scenario: &Scenario, at: &str, expect: Expect) -> Result<PutReport, PutError> {
    removing(scenario, at, Removing::File(expect), Unmatched::Refuse)
}

fn removing(
    scenario: &Scenario,
    at: &str,
    removing: Removing,
    unmatched: Unmatched,
) -> Result<PutReport, PutError> {
    open(scenario).remove_waiting(&path(at), removing, unmatched, LOCK_WAIT)
}

fn paths(report: &PutReport) -> Vec<String> {
    (report.removed.iter())
        .map(|removed| removed.path.to_string())
        .collect()
}

fn code(result: Result<PutReport, PutError>) -> Code {
    result.unwrap_err().code()
}

fn blob(scenario: &Scenario, physical: &str) -> ObjectId {
    open(scenario)
        .repository()
        .blob_id(&file(scenario, physical).unwrap())
}

#[test]
fn a_record_and_a_file_are_removed_and_reported_and_a_missing_path_is_not_found() {
    let scenario = board();
    scenario.write("notes/n.md", Item::File("n"));
    let held = blob(&scenario, "projects/p.json");
    let report = rm(&scenario, "/projects/p", Expect::Any).unwrap();
    assert_eq!(file(&scenario, "projects/p.json"), None);
    assert!(report.writes.is_empty());
    assert_eq!(report.removed.len(), 1);
    assert_eq!(report.removed[0].path, path("/projects/p"));
    assert_eq!(report.removed[0].blob, held);
    assert_eq!(report.commit, None);
    let removed = rm(&scenario, "/notes/n.md", Expect::Any).unwrap();
    assert_eq!(file(&scenario, "notes/n.md"), None);
    // No commit held the file, and git still restores it from the blob.
    let restored = scenario.stored(removed.removed[0].blob);
    assert_eq!(restored.as_deref(), Some(&b"n"[..]));
    assert_eq!(
        code(rm(&scenario, "/projects/p", Expect::Any)),
        Code::PathNotFound
    );
}

#[test]
fn a_removal_commits_only_its_own_path_onto_head() {
    let scenario = committing();
    put(&scenario, "/tasks/a", Expect::Absent, &task("")).unwrap();
    put(&scenario, "/tasks/b", Expect::Absent, &task("")).unwrap();
    let report = rm(&scenario, "/tasks/a", Expect::Any).unwrap();
    let head = scenario.head().unwrap();
    assert_eq!(report.commit, Some(head.id));
    assert_eq!(head.message, "board rm /tasks/a");
    assert_eq!(head.files.keys().collect::<Vec<_>>(), ["tasks/b.json"]);
    assert_eq!(file(&scenario, "notes.md").unwrap(), b"not committed");
}

#[test]
fn a_removal_whose_expect_differs_is_stale_and_its_twin_is_admitted() {
    let scenario = board();
    let held = blob(&scenario, "projects/p.json");
    let other = blob(&scenario, ".board/board.json");
    for stale in [Expect::Absent, Expect::Blob(other)] {
        match rm(&scenario, "/projects/p", stale).unwrap_err() {
            PutError::Stale { found, .. } => assert_eq!(found, Some(held)),
            other => panic!("not stale: {other}"),
        }
    }
    assert!(file(&scenario, "projects/p.json").is_some());
    assert!(rm(&scenario, "/projects/p", Expect::Blob(held)).is_ok());
}

#[test]
fn a_directory_is_refused_and_a_file_in_it_is_admitted() {
    let scenario = board();
    assert_eq!(
        code(rm(&scenario, "/projects", Expect::Any)),
        Code::RmDirectory
    );
    assert!(file(&scenario, "projects/p.json").is_some());
    assert!(rm(&scenario, "/projects/p", Expect::Any).is_ok());
}

#[test]
fn a_removal_through_a_mount_is_refused_naming_it_and_of_the_mount_is_admitted() {
    let library = Scenario::new();
    let commit = library.commit(&[("x.json", "{}")]);
    let scenario = board();
    scenario.write(
        "vendor.json",
        Item::File(&format!(
            r#"{{"$type": "board/mount/v1", "repo": "{}", "follow": "main", "commit": "{commit}"}}"#,
            library.path()
        )),
    );
    let refused = rm(&scenario, "/vendor/x", Expect::Any).unwrap_err();
    assert_eq!(refused.code(), Code::PutThroughMount);
    assert_eq!(refused.fields(), Fields::at(&path("/vendor")));
    assert!(rm(&scenario, "/vendor", Expect::Any).is_ok());
    assert_eq!(file(&scenario, "vendor.json"), None);
}

#[test]
fn a_removal_through_a_link_removes_the_target_and_of_the_link_removes_the_link() {
    let scenario = board();
    scenario.write(
        "l.json",
        Item::File(r#"{"$type": "board/mount/v1", "path": "/projects"}"#),
    );
    assert!(rm(&scenario, "/l/p", Expect::Any).is_ok());
    assert_eq!(file(&scenario, "projects/p.json"), None);
    assert!(rm(&scenario, "/l", Expect::Any).is_ok());
    assert_eq!(file(&scenario, "l.json"), None);
}

fn batch(scenario: &Scenario, input: &str) -> Result<PutReport, PutError> {
    open(scenario).put_batch(&mut input.as_bytes())
}

fn removal(at: &str) -> String {
    format!(r#"{{"path": "{at}", "remove": true}}"#) + "\n"
}

/// The code of each refused line of a refused batch, by line number.
fn refused_lines(result: Result<PutReport, PutError>) -> Vec<(usize, Code)> {
    match result.unwrap_err() {
        PutError::Batch { refusals, .. } => refusals
            .iter()
            .map(|lined| (lined.line, lined.code))
            .collect(),
        other => panic!("not a batch refusal: {other}"),
    }
}

#[test]
fn a_batch_writes_and_removes_in_one_commit() {
    let scenario = committing();
    put(&scenario, "/tasks/a", Expect::Absent, &task("")).unwrap();
    let write = format!(r#"{{"path": "/tasks/b", "record": {}}}"#, task("")) + "\n";
    let report = batch(&scenario, &(write + &removal("/tasks/a"))).unwrap();
    let head = scenario.head().unwrap();
    assert_eq!(report.commit, Some(head.id));
    assert_eq!(head.message, "board put --batch (1 records, 1 removals)");
    assert_eq!(head.files.keys().collect::<Vec<_>>(), ["tasks/b.json"]);
    assert_eq!(report.removed[0].path, path("/tasks/a"));
}

#[test]
fn a_removal_line_that_also_writes_or_meets_a_write_of_its_file_is_refused_and_alone_is_admitted() {
    let scenario = board();
    let both = format!(
        r#"{{"path": "/tasks/a", "remove": true, "record": {}}}"#,
        task("")
    ) + "\n";
    let write = format!(
        r#"{{"path": "/projects/p", "record": {}}}"#,
        r#"{"$type": "lr/project/v1"}"#
    ) + "\n";
    let input = both + &write + &removal("/projects/p");
    let lines = refused_lines(batch(&scenario, &input));
    assert_eq!(lines, [(1, Code::PutInvalid), (3, Code::PutInvalid)]);
    assert!(file(&scenario, "projects/p.json").is_some());
    assert!(batch(&scenario, &removal("/projects/p")).is_ok());
    assert_eq!(file(&scenario, "projects/p.json"), None);
}

#[test]
fn a_removal_line_through_a_mount_or_of_a_directory_is_refused_and_of_a_file_is_admitted() {
    let library = Scenario::new();
    let commit = library.commit(&[("x.json", "{}")]);
    let scenario = board();
    scenario.write(
        "vendor.json",
        Item::File(&format!(
            r#"{{"$type": "board/mount/v1", "repo": "{}", "follow": "main", "commit": "{commit}"}}"#,
            library.path()
        )),
    );
    let input = removal("/projects") + &removal("/vendor/x");
    assert_eq!(
        refused_lines(batch(&scenario, &input)),
        [(1, Code::RmDirectory), (2, Code::PutThroughMount)]
    );
    assert!(batch(&scenario, &removal("/projects/p")).is_ok());
}

#[test]
fn a_recursive_removal_removes_every_file_beneath_in_one_commit_and_leaves_the_rest() {
    let scenario = committing();
    for at in ["/tasks/a", "/tasks/b", "/projects/c"] {
        put(&scenario, at, Expect::Absent, &task("")).unwrap();
    }
    let report = removing(&scenario, "/tasks", Removing::Beneath, Unmatched::Refuse).unwrap();
    assert_eq!(paths(&report), ["/tasks/a", "/tasks/b"]);
    let head = scenario.head().unwrap();
    assert_eq!(report.commit, Some(head.id));
    assert_eq!(head.message, "board rm -r /tasks");
    assert_eq!(head.files.keys().collect::<Vec<_>>(), ["projects/c.json"]);
    let one = removing(
        &scenario,
        "/projects/c",
        Removing::Beneath,
        Unmatched::Refuse,
    )
    .unwrap();
    assert_eq!(paths(&one), ["/projects/c"]);
}

#[test]
fn a_recursive_removal_over_a_symlink_removes_nothing_and_beside_it_removes_all() {
    let scenario = board();
    scenario
        .write("tasks/a.json", Item::File(&task("")))
        .write("tasks/s.json", Item::Symlink("a.json"))
        .write("notes/n.md", Item::File("n"));
    let refused = removing(&scenario, "/tasks", Removing::Beneath, Unmatched::Refuse);
    assert_eq!(code(refused), Code::PutSymlink);
    assert!(file(&scenario, "tasks/a.json").is_some());
    let report = removing(&scenario, "/notes", Removing::Beneath, Unmatched::Refuse).unwrap();
    assert_eq!(paths(&report), ["/notes/n.md"]);
}

#[test]
fn an_absent_path_with_ignore_unmatch_is_reported_absent_and_without_it_is_not_found() {
    let scenario = board();
    for how in [Removing::File(Expect::Any), Removing::Beneath] {
        let report = removing(&scenario, "/tasks/zz", how, Unmatched::Ignore).unwrap();
        assert_eq!(report.absent, [path("/tasks/zz")]);
        assert!(report.removed.is_empty());
        assert_eq!(report.commit, None);
        let refused = removing(&scenario, "/tasks/zz", how, Unmatched::Refuse);
        assert_eq!(code(refused), Code::PathNotFound);
    }
}

#[test]
fn ignore_unmatch_still_refuses_a_directory_or_a_stale_expect_and_removes_a_file() {
    let scenario = board();
    let ignore = |at: &str, how| removing(&scenario, at, how, Unmatched::Ignore);
    let file_with = |expect| Removing::File(expect);
    assert_eq!(
        code(ignore("/projects", file_with(Expect::Any))),
        Code::RmDirectory
    );
    assert_eq!(
        code(ignore("/projects/p", file_with(Expect::Absent))),
        Code::PutStale
    );
    let report = ignore("/projects/p", file_with(Expect::Any)).unwrap();
    assert_eq!(paths(&report), ["/projects/p"]);
    assert!(report.absent.is_empty());
}

#[test]
fn a_batch_removal_line_with_ignore_unmatch_reports_an_absent_path_and_one_without_remove_is_invalid()
 {
    let scenario = board();
    let ignoring =
        |at: &str| format!(r#"{{"path": "{at}", "remove": true, "ignore_unmatch": true}}"#) + "\n";
    let report = batch(
        &scenario,
        &(ignoring("/tasks/zz") + &ignoring("/projects/p")),
    )
    .unwrap();
    assert_eq!(report.absent, [path("/tasks/zz")]);
    assert_eq!(paths(&report), ["/projects/p"]);
    let writing = format!(
        r#"{{"path": "/tasks/a", "record": {}, "ignore_unmatch": true}}"#,
        task("")
    ) + "\n";
    assert_eq!(
        refused_lines(batch(&scenario, &(writing + &removal("/tasks/zz")))),
        [(1, Code::PutInvalid), (2, Code::PathNotFound)]
    );
}
