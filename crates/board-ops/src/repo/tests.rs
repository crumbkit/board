//! Writing a repository record once, at the path its remote's identity
//! gives, and the put line later writers add to their own batches.

use board_model::{Code, LogicalPath, repo_identity};
use board_scenario::{Item, Scenario};
use serde_json::{Value, json};

use super::{RepoAdded, ensure};
use crate::tests::open;

fn remote(text: &str) -> board_model::Remote {
    repo_identity(text).unwrap()
}

/// The test board, committing each put as a test identity, since CI has
/// no git identity of its own.
fn board() -> Scenario {
    let scenario = crate::tests::board();
    scenario.identified();
    scenario
}

/// The file at `physical` in `scenario`'s working tree, if any.
fn file(scenario: &Scenario, physical: &str) -> Option<Value> {
    let bytes = std::fs::read(scenario.workdir().join(physical)).ok()?;
    Some(serde_json::from_slice(&bytes).unwrap())
}

#[test]
fn ensure_gives_a_put_line_that_expects_nothing_for_a_missing_record_and_nothing_for_a_present_one()
{
    let scenario = board();
    let board = open(&scenario);
    let line = ensure(
        &board,
        &LogicalPath::root(),
        &remote("git@github.com:CrumbKit/Board.git"),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        serde_json::to_value(&line).unwrap(),
        json!({
            "path": "/repositories/crumbkit/board",
            "expect": "none",
            "record": {"$type": "board/repo/v2", "remote": "crumbkit/board"}
        })
    );
    let batch = format!("{}\n", serde_json::to_string(&line).unwrap());
    board.put_batch(&mut batch.as_bytes()).unwrap();
    assert_eq!(
        ensure(&board, &LogicalPath::root(), &remote("crumbkit/board")).unwrap(),
        None
    );
}

#[test]
fn two_batches_that_ensure_one_record_leave_one_record_and_the_second_is_stale() {
    let scenario = board();
    let board = open(&scenario);
    let identity = remote("crumbkit/board");
    let first = ensure(&board, &LogicalPath::root(), &identity)
        .unwrap()
        .unwrap();
    let second = ensure(&board, &LogicalPath::root(), &identity)
        .unwrap()
        .unwrap();
    let batch = |line| format!("{}\n", serde_json::to_string(&line).unwrap());
    board.put_batch(&mut batch(first).as_bytes()).unwrap();
    let head = scenario.head().unwrap().id;
    let error = board.put_batch(&mut batch(second).as_bytes()).unwrap_err();
    assert_eq!(error.code(), Code::PutStale);
    assert_eq!(scenario.head().unwrap().id, head);
    let records: Vec<_> = (scenario.head().unwrap().files.into_keys())
        .filter(|path| path.starts_with("repositories/"))
        .collect();
    assert_eq!(records, ["repositories/crumbkit/board.json"]);
}

#[test]
fn repo_add_writes_the_record_in_one_commit_and_a_second_call_is_unchanged_and_commits_nothing() {
    let scenario = board();
    let board = open(&scenario);
    let added = board.repo_add("git@github.com:CrumbKit/Board.git").unwrap();
    let RepoAdded::Wrote { remote, report } = added else {
        panic!("{added:?}");
    };
    assert_eq!(remote.path().as_str(), "/repositories/crumbkit/board");
    let head = scenario.head().unwrap().id;
    assert_eq!(report.commit, Some(head));
    assert_eq!(
        file(&scenario, "repositories/crumbkit/board.json"),
        Some(json!({"$type": "board/repo/v2", "remote": "crumbkit/board"}))
    );
    let again = board
        .repo_add("https://github.com/crumbkit/board.git")
        .unwrap();
    assert_eq!(again, RepoAdded::Unchanged { remote });
    assert_eq!(scenario.head().unwrap().id, head);
    let report = board.check(None, None).unwrap();
    assert!(
        report.entries.iter().all(|entry| entry.detail.is_none()),
        "{report:?}"
    );
}

#[test]
fn repo_add_refuses_a_local_or_too_long_remote_and_writes_nothing() {
    let scenario = board();
    let board = open(&scenario);
    let long = format!("https://gitlab.com/group/{}.git", "a".repeat(251));
    for (given, code) in [
        ("/srv/git/app.git", Code::RepoLocalRemote),
        ("file:///srv/git/app.git", Code::RepoLocalRemote),
        ("../app", Code::RepoLocalRemote),
        (long.as_str(), Code::RepoTooLong),
    ] {
        let error = board.repo_add(given).unwrap_err();
        assert_eq!(error.code(), code, "{given}");
        assert!(error.to_string().starts_with("the remote "), "{error}");
    }
    assert!(scenario.head().is_none());
    assert!(!scenario.workdir().join("repositories").exists());
}

#[test]
fn repo_add_from_a_clone_reads_its_origin_and_one_with_no_origin_or_a_local_one_is_refused() {
    let scenario = board();
    let board = open(&scenario);
    let clone = Scenario::new();
    clone.commit(&[("a.md", "one")]);
    let error = board.repo_add_from(clone.workdir()).unwrap_err();
    assert_eq!(error.code(), Code::MountNoRemote);
    assert!(scenario.head().is_none());
    clone.write(
        ".git/config",
        Item::File("[remote \"origin\"]\n\turl = /srv/git/app.git\n"),
    );
    let error = board.repo_add_from(clone.workdir()).unwrap_err();
    assert_eq!(error.code(), Code::RepoLocalRemote);
    assert_eq!(
        error.to_string(),
        "the origin /srv/git/app.git is a local path, which names no repository outside this machine"
    );
    clone.write(
        ".git/config",
        Item::File("[remote \"origin\"]\n\turl = git@gitlab.com:Example-Org/App.git\n"),
    );
    let added = board.repo_add_from(clone.workdir()).unwrap();
    let RepoAdded::Wrote { remote, .. } = added else {
        panic!("{added:?}");
    };
    assert_eq!(remote.as_str(), "gitlab.com/Example-Org/App");
    assert!(file(&scenario, "repositories/gitlab.com/Example-Org/App.json").is_some());
}

#[test]
fn ensure_through_a_mount_gives_a_line_under_the_mount_and_reads_the_record_there() {
    let team = Scenario::board();
    let first = team.commit(&[(".board/board.json", r#"{"$type": "board/config/v1"}"#)]);
    let scenario = board();
    scenario.write(
        "team.json",
        Item::File(&board_scenario::seed::mount_of(&team.path(), first)),
    );
    let board = open(&scenario);
    let team_path: LogicalPath = "/team".parse().unwrap();
    let identity = remote("crumbkit/board");
    let line = ensure(&board, &team_path, &identity).unwrap().unwrap();
    assert_eq!(
        serde_json::to_value(&line).unwrap()["path"],
        "/team/repositories/crumbkit/board"
    );
    scenario.write(
        "repositories/crumbkit/board.json",
        Item::File(r#"{"$type": "board/repo/v2", "remote": "crumbkit/board"}"#),
    );
    assert!(ensure(&board, &team_path, &identity).unwrap().is_some());
    assert_eq!(
        ensure(&board, &LogicalPath::root(), &identity).unwrap(),
        None
    );
    let second = team.commit(&[
        (".board/board.json", r#"{"$type": "board/config/v1"}"#),
        (
            "repositories/crumbkit/board.json",
            r#"{"$type": "board/repo/v2", "remote": "crumbkit/board"}"#,
        ),
    ]);
    scenario.write(
        "team.json",
        Item::File(&board_scenario::seed::mount_of(&team.path(), second)),
    );
    assert_eq!(ensure(&board, &team_path, &identity).unwrap(), None);
}

#[test]
fn an_invalid_or_misplaced_record_at_the_path_is_not_present_and_repo_add_refuses_it() {
    let identity = remote("crumbkit/board");
    for (held, code) in [
        (
            r#"{"$type": "board/repo/v2", "remote": "CrumbKit/Board"}"#,
            Code::RepoInvalid,
        ),
        (
            r#"{"$type": "board/repo/v1", "remote": "crumbkit/board"}"#,
            Code::RepoInvalid,
        ),
        ("{not json", Code::RepoInvalid),
        (
            r#"{"$type": "board/repo/v2", "remote": "crumbkit/other"}"#,
            Code::RepoPathMismatch,
        ),
    ] {
        let scenario = board();
        scenario.write("repositories/crumbkit/board.json", Item::File(held));
        let board = open(&scenario);
        let error = ensure(&board, &LogicalPath::root(), &identity).unwrap_err();
        assert_eq!(error.code(), code, "{held}: {error}");
        assert!(
            error.to_string().starts_with(
                "/repositories/crumbkit/board holds no valid record of crumbkit/board: "
            ),
            "{error}"
        );
        let error = board.repo_add("crumbkit/board").unwrap_err();
        assert_eq!(error.code(), code, "{held}");
        assert!(
            (error.refusal().hint.as_deref()).is_some_and(|hint| hint.contains("board rm")),
            "{:?}",
            error.refusal()
        );
        assert!(scenario.head().is_none(), "{held}");
    }
}

#[test]
fn repo_add_from_a_clone_whose_origin_is_a_relative_path_refuses_it_as_local() {
    let scenario = board();
    let board = open(&scenario);
    let clone = Scenario::new();
    clone.commit(&[("a.md", "one")]);
    clone.write(
        ".git/config",
        Item::File("[remote \"origin\"]\n\turl = upstream/app\n"),
    );
    let error = board.repo_add_from(clone.workdir()).unwrap_err();
    assert_eq!(error.code(), Code::RepoLocalRemote);
    assert_eq!(
        error.to_string(),
        "the origin upstream/app is a local path, which names no repository outside this machine"
    );
    assert!(scenario.head().is_none());
    assert!(board.repo_add("upstream/app").is_ok());
}
