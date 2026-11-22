//! `board repo add` through the binary: the record it writes once, what
//! `check` says of it, and the remotes it refuses.

use board_scenario::{Item, Scenario};
use serde_json::{Value, json};

fn report(output: &crate::report::Output) -> Value {
    super::json(output)
}

use super::{board_in, committing, text};

/// Asserts that `output` was refused with `code` and an error line that
/// starts with `error`, and that `scenario` holds no new commit since `head`.
fn refused(output: &crate::report::Output, code: &str, error: &str) {
    let stderr = text(&output.stderr);
    assert_eq!(output.code, 1, "{stderr}");
    assert!(stderr.starts_with(&format!("error: {error}")), "{stderr}");
    assert!(stderr.contains(&format!("\n  code:  {code}\n")), "{stderr}");
}

#[test]
fn repo_add_writes_the_record_in_one_commit_check_passes_and_a_second_call_is_unchanged() {
    let scenario = committing();
    let added = board_in(
        scenario.workdir(),
        &["repo", "add", "git@github.com:CrumbKit/Board.git"],
    );
    assert_eq!(added.code, 0, "{}", text(&added.stderr));
    let head = scenario.head().unwrap();
    assert_eq!(
        text(&added.stdout),
        format!("wrote /repositories/crumbkit/board\ncommit {}\n", head.id)
    );
    let record: Value =
        serde_json::from_slice(&head.files["repositories/crumbkit/board.json"]).unwrap();
    assert_eq!(
        record,
        json!({"$type": "board/repo/v2", "remote": "crumbkit/board"})
    );
    let checked = board_in(scenario.workdir(), &["check"]);
    assert_eq!(checked.code, 0, "{}", text(&checked.stdout));
    let again = board_in(
        scenario.workdir(),
        &["--json", "repo", "add", "https://github.com/crumbkit/board"],
    );
    let again = report(&again);
    assert_eq!(again["schema"], "board/repo-add-report/v2");
    assert_eq!(
        again["data"],
        json!({"path": "/repositories/crumbkit/board", "remote": "crumbkit/board", "wrote": false, "commit": null})
    );
    assert_eq!(scenario.head().unwrap().id, head.id);
}

#[test]
fn repo_add_of_a_local_path_or_a_too_long_name_is_refused_and_writes_nothing_and_a_hosted_twin_writes()
 {
    let scenario = committing();
    let local = board_in(scenario.workdir(), &["repo", "add", "/srv/git/app.git"]);
    refused(
        &local,
        "board::repo::local_remote",
        "the remote /srv/git/app.git is a local path, which names no repository outside this machine",
    );
    let long = format!("https://gitlab.com/group/{}.git", "a".repeat(251));
    let too_long = board_in(scenario.workdir(), &["repo", "add", &long]);
    refused(
        &too_long,
        "board::repo::too_long",
        "the remote https://gitlab.com/group/",
    );
    assert!(scenario.head().is_none());
    let hosted = board_in(
        scenario.workdir(),
        &["repo", "add", "https://gitlab.com/group/app.git"],
    );
    assert_eq!(
        text(&hosted.stdout).lines().next(),
        Some("wrote /repositories/gitlab.com/group/app")
    );
}

#[test]
fn repo_add_from_a_clone_with_no_origin_is_refused_and_one_with_an_origin_writes_its_record() {
    let scenario = committing();
    let clone = Scenario::new();
    clone.commit(&[("a.md", "one")]);
    let from = clone.workdir().to_string_lossy().into_owned();
    let refused_run = board_in(scenario.workdir(), &["repo", "add", "--from", &from]);
    refused(&refused_run, "board::mount::no_remote", &from);
    assert!(scenario.head().is_none());
    clone.write(
        ".git/config",
        Item::File("[remote \"origin\"]\n\turl = git@github.com:crumbkit/app.git\n"),
    );
    let added = board_in(scenario.workdir(), &["repo", "add", "--from", &from]);
    assert_eq!(
        text(&added.stdout).lines().next(),
        Some("wrote /repositories/crumbkit/app")
    );
}

#[test]
fn check_degrades_a_misplaced_and_an_invalid_repository_record_and_names_the_right_path_and_value()
{
    let scenario = committing();
    scenario
        .write(
            "repositories/crumbkit/other.json",
            Item::File(r#"{"$type": "board/repo/v2", "remote": "crumbkit/board"}"#),
        )
        .write(
            "repositories/acme/app.json",
            Item::File(r#"{"$type": "board/repo/v2", "remote": "Acme/App"}"#),
        );
    let checked = report(&board_in(scenario.workdir(), &["--json", "check"]));
    let entries = checked["data"].as_array().unwrap();
    let find = |path: &str| {
        (entries.iter())
            .find(|entry| entry["logical"] == path)
            .unwrap_or_else(|| panic!("{path}: {checked}"))
    };
    let misplaced = find("/repositories/crumbkit/other");
    assert_eq!(misplaced["code"], "board::repo::path_mismatch");
    assert!(
        misplaced
            .to_string()
            .contains("belongs at /repositories/crumbkit/board"),
        "{misplaced}"
    );
    let invalid = find("/repositories/acme/app");
    assert_eq!(invalid["code"], "board::repo::invalid");
    assert!(
        invalid
            .to_string()
            .contains("its identity form is `acme/app`"),
        "{invalid}"
    );
}
