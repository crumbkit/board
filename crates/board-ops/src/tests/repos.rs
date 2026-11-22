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

fn repository(remote: &str) -> String {
    format!(r#"{{"$type": "board/repo/v2", "remote": "{remote}"}}"#)
}

#[test]
fn a_repository_record_at_its_identity_path_is_ok_and_one_elsewhere_names_the_path_it_belongs_at() {
    let scenario = board();
    scenario
        .write(
            "repositories/crumbkit/board.json",
            Item::File(&repository("crumbkit/board")),
        )
        .write(
            "repositories/crumbkit/other.json",
            Item::File(&repository("crumbkit/board")),
        )
        .write(
            "repositories/Acme/App.json",
            Item::File(&repository("acme/app")),
        );
    assert_eq!(
        status(&scenario, "repositories/crumbkit/board.json"),
        Status::Ok
    );
    for (misplaced, remote) in [
        ("repositories/crumbkit/other.json", "crumbkit/board"),
        ("repositories/Acme/App.json", "acme/app"),
    ] {
        assert_eq!(
            status(&scenario, misplaced),
            Status::Degraded(Code::RepoPathMismatch),
            "{misplaced}"
        );
        let (_, reason, _) = detail(&scenario, misplaced).unwrap();
        assert_eq!(
            reason,
            format!(
                "names the repository {remote}, whose record belongs at /repositories/{remote}"
            )
        );
    }
}

#[test]
fn a_repository_record_not_in_identity_form_or_with_another_field_is_invalid_and_names_the_identity()
 {
    let scenario = board();
    scenario
        .write(
            "repositories/crumbkit/board.json",
            Item::File(&repository("CrumbKit/Board")),
        )
        .write(
            "repositories/crumbkit/app.json",
            Item::File(r#"{"$type": "board/repo/v2", "remote": "crumbkit/app", "branch": "main"}"#),
        )
        .write(
            "repositories/crumbkit/ok.json",
            Item::File(&repository("crumbkit/ok")),
        );
    let invalid = Status::Degraded(Code::RepoInvalid);
    assert_eq!(
        status(&scenario, "repositories/crumbkit/board.json"),
        invalid
    );
    assert_eq!(status(&scenario, "repositories/crumbkit/app.json"), invalid);
    assert_eq!(
        status(&scenario, "repositories/crumbkit/ok.json"),
        Status::Ok
    );
    let (_, reason, _) = detail(&scenario, "repositories/crumbkit/board.json").unwrap();
    assert!(
        reason.contains("its identity form is `crumbkit/board`"),
        "{reason}"
    );
    let (_, reason, _) = detail(&scenario, "repositories/crumbkit/app.json").unwrap();
    assert!(reason.contains("unknown field `branch`"), "{reason}");
}

#[test]
fn a_reference_to_a_repository_record_in_a_mounted_board_dangles_until_that_board_holds_its_own_copy()
 {
    let pull = r#"{"type": "object", "properties": {"repository": {"type": "string", "format": "board-ref", "x-board-type": "board/repo/v2"}}}"#;
    let team_files = [
        (".board/board.json", r#"{"$type": "board/config/v1"}"#),
        (".board/types/lr/pull/v1.schema.json", pull),
        (
            "pulls/a.json",
            r#"{"$type": "lr/pull/v1", "repository": "/repositories/crumbkit/board"}"#,
        ),
    ];
    let team = Scenario::new();
    let first = team.commit(&team_files);
    let scenario = board();
    scenario
        .write(
            "repositories/crumbkit/board.json",
            Item::File(&repository("crumbkit/board")),
        )
        .write(
            "team.json",
            Item::File(&board_scenario::seed::mount_of(&team.path(), first)),
        );
    assert_eq!(
        status(&scenario, "pulls/a.json"),
        Status::Degraded(Code::RefDangling)
    );
    let copy = repository("crumbkit/board");
    let mut files = team_files.to_vec();
    files.push(("repositories/crumbkit/board.json", &copy));
    files.push(("repositories/crumbkit/other.json", &copy));
    let second = team.commit(&files);
    scenario.write(
        "team.json",
        Item::File(&board_scenario::seed::mount_of(&team.path(), second)),
    );
    assert_eq!(status(&scenario, "pulls/a.json"), Status::Ok);
    let report = check(&scenario);
    let at = |logical: &str| {
        (report.entries.iter())
            .find(|entry| entry.logical.as_ref().map(LogicalPath::as_str) == Some(logical))
            .unwrap_or_else(|| panic!("{logical} is not an entry"))
    };
    assert_eq!(at("/team/repositories/crumbkit/board").status, Status::Ok);
    let misplaced = at("/team/repositories/crumbkit/other");
    assert_eq!(misplaced.status, Status::Degraded(Code::RepoPathMismatch));
    assert_eq!(
        misplaced.detail.as_ref().unwrap().reason,
        "names the repository crumbkit/board, whose record belongs at /team/repositories/crumbkit/board"
    );
}
