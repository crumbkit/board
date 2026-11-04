//! Repo records: one record per repository, with a checkout or without
//! (docs/architecture/paths/repos.md).

use super::*;

fn repo(remote: &str) -> String {
    format!(r#"{{"$type": "board/repo/v1", "remote": "{remote}"}}"#)
}

/// A repo record of `remote` that mounts `library` at its one commit.
fn checkout(remote: &str, library: &Scenario) -> String {
    let commit = library.commit(&[("x.json", "1")]);
    format!(
        r#"{{"$type": "board/repo/v1", "remote": "{remote}", "checkout": {{"repo": "{}", "follow": "main", "commit": "{commit}"}}}}"#,
        library.path()
    )
}

#[test]
fn two_repo_records_of_one_remote_are_each_a_duplicate_and_records_of_two_remotes_are_not() {
    let library = Scenario::new();
    let scenario = board();
    scenario
        .write("repos/a.json", Item::File(&repo("example-org/a")))
        .write(
            "work/a.json",
            Item::File(&checkout("example-org/a", &library)),
        )
        .write("repos/b.json", Item::File(&repo("example-org/b")));
    let duplicate = Status::Degraded(Code::RepoDuplicateRemote);
    assert_eq!(status(&scenario, "repos/a.json"), duplicate);
    assert_eq!(status(&scenario, "work/a.json"), duplicate);
    assert_eq!(status(&scenario, "repos/b.json"), Status::Ok);
    let (_, reason, _) = detail(&scenario, "repos/a.json").unwrap();
    assert_eq!(
        reason,
        "names the repository example-org/a, which /work/a also names"
    );
    scenario.write(
        "work/a.json",
        Item::File(&checkout("example-org/c", &library)),
    );
    assert_eq!(status(&scenario, "repos/a.json"), Status::Ok);
    assert_eq!(status(&scenario, "work/a.json"), Status::Ok);
}

#[test]
fn a_check_scoped_to_one_duplicate_still_reports_it_and_one_scoped_to_its_twin_does_not() {
    let scenario = board();
    scenario
        .write("repos/a.json", Item::File(&repo("example-org/a")))
        .write("work/a.json", Item::File(&repo("example-org/a")))
        .write("solo/b.json", Item::File(&repo("example-org/b")));
    let scoped = |at: &str| {
        let scope: LogicalPath = at.parse().unwrap();
        let report = open(&scenario).check(Some(&scope), None).unwrap();
        report
            .entries
            .into_iter()
            .map(|entry| entry.status)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        scoped("/repos"),
        [Status::Degraded(Code::RepoDuplicateRemote)]
    );
    assert_eq!(scoped("/solo"), [Status::Ok]);
}

#[test]
fn a_repo_record_without_a_checkout_is_a_valid_reference_target_with_no_children() {
    let scenario = board();
    scenario
        .write("repos/lib.json", Item::File(&repo("example-org/lib")))
        .write(
            "tasks/t.json",
            Item::File(&task(r#", "see": "/repos/lib""#)),
        )
        .write(
            "tasks/u.json",
            Item::File(&task(r#", "see": "/repos/lib/docs""#)),
        );
    assert_eq!(status(&scenario, "repos/lib.json"), Status::Ok);
    assert_eq!(status(&scenario, "tasks/t.json"), Status::Ok);
    assert_eq!(
        status(&scenario, "tasks/u.json"),
        Status::Degraded(Code::RefDangling)
    );
    let board = open(&scenario);
    let below = board.get(&"/repos/lib/docs".parse().unwrap()).unwrap_err();
    assert_eq!(below.code(), Code::PathNotFound);
    assert!(board.get(&"/repos/lib".parse().unwrap()).is_ok());
}

#[test]
fn a_repo_record_with_a_checkout_reads_the_mounted_tree_beneath_it() {
    let library = Scenario::new();
    let scenario = board();
    scenario.write(
        "repos/lib.json",
        Item::File(&checkout("example-org/lib", &library)),
    );
    scenario.write(
        "tasks/t.json",
        Item::File(&task(r#", "see": "/repos/lib/x""#)),
    );
    assert_eq!(status(&scenario, "repos/lib.json"), Status::Ok);
    assert_eq!(status(&scenario, "tasks/t.json"), Status::Ok);
}

#[test]
fn an_invalid_repo_record_is_degraded_and_its_valid_twin_is_ok() {
    let scenario = board();
    scenario
        .write(
            "repos/bad.json",
            Item::File(r#"{"$type": "board/repo/v1", "name": "x"}"#),
        )
        .write("repos/good.json", Item::File(&repo("example-org/x")));
    assert_eq!(
        status(&scenario, "repos/bad.json"),
        Status::Degraded(Code::MountInvalid)
    );
    assert_eq!(status(&scenario, "repos/good.json"), Status::Ok);
}
