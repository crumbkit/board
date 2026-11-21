//! The second round of a URI's lookup: when no board listed under `boards`
//! has the URI's remote, Board looks at the personal board, the board that
//! holds the start directory and the boards either round mounts
//! (docs/architecture/paths/uris.md, Resolution).

use std::path::{Path, PathBuf};

use board_model::Refusal;
use board_scenario::seed::mount_of;
use board_scenario::{Item, Scenario};
use serde_json::json;

use super::super::KnownBoards;
use crate::tests::board;

const URI: &str = "board://acme/team-board/tasks/a";

/// A board whose `origin` is `url`.
fn originated(url: &str) -> Scenario {
    let scenario = board();
    let remote = format!("[remote \"origin\"]\n\turl = {url}\n");
    scenario.write(".git/config", Item::Appended(&remote));
    scenario
}

/// A board with no origin that mounts each of `clones` at `/repos/<n>`.
fn mounting(clones: &[&Scenario]) -> Scenario {
    let scenario = board();
    for (at, clone) in clones.iter().enumerate() {
        let commit = clone.commit(&[("x.json", "1")]);
        let mount = mount_of(&clone.path(), commit);
        scenario.write(&format!("repos/{at}.json"), Item::File(&mount));
    }
    scenario
}

/// The known boards file holding `value`, read as an adapter reads it,
/// and the directory that holds it.
fn known(value: &serde_json::Value) -> (KnownBoards, Scenario) {
    let xdg = Scenario::new();
    xdg.write("board/config.json", Item::File(&value.to_string()));
    let file = xdg.workdir().join("board/config.json");
    (
        KnownBoards::parse(&file, std::fs::read(&file)).unwrap(),
        xdg,
    )
}

/// The board `uri` names and the record's path, from `start`, or the
/// refusal.
fn by_uri(
    known: &KnownBoards,
    uri: &str,
    start: Option<&Path>,
) -> Result<(PathBuf, String), Box<Refusal>> {
    let cache = Scenario::new();
    let (root, path) = known.by_uri_from(uri, None, start, Some(cache.workdir()))?;
    Ok((root, path.to_string()))
}

/// `root` as the path of the repository it names.
fn real(root: &Path) -> PathBuf {
    root.canonicalize().unwrap()
}

#[test]
fn a_uri_resolves_through_the_personal_boards_mount_and_without_a_personal_board_is_not_found() {
    let team = originated("git@github.com:acme/team-board.git");
    let personal = mounting(&[&team]);
    let (named, _xdg) = known(&json!({"personal": personal.path()}));
    let (root, path) = by_uri(&named, URI, None).unwrap();
    assert_eq!(real(&root), real(team.workdir()));
    assert_eq!(path, "/tasks/a");
    let (empty, _xdg) = known(&json!({}));
    let lost = *by_uri(&empty, URI, None).unwrap_err();
    assert_eq!(lost.code.as_str(), "board::uri::not_found");
    let hint = lost.hint.unwrap();
    assert!(hint.contains("`board -C <dir> get /tasks/a`"), "{hint}");
}

#[test]
fn a_uri_resolves_in_the_board_holding_the_start_or_one_it_mounts_and_not_from_elsewhere() {
    let team = originated("git@github.com:acme/team-board.git");
    let mine = mounting(&[&team]);
    let (empty, _xdg) = known(&json!({}));
    for start in [mine.workdir(), team.workdir()] {
        let (root, _) = by_uri(&empty, URI, Some(start)).unwrap();
        assert_eq!(real(&root), real(team.workdir()));
    }
    let outside = Scenario::new();
    let lost = *by_uri(&empty, URI, Some(&outside.workdir().join("nowhere"))).unwrap_err();
    assert_eq!(lost.code.as_str(), "board::uri::not_found");
}

#[test]
fn a_listed_board_with_the_remote_wins_over_a_mounted_clone_with_a_longer_one() {
    let outer = originated("https://git.example.com/team");
    let inner = originated("https://git.example.com/team/board");
    let personal = mounting(&[&inner]);
    let (named, _xdg) = known(&json!({"boards": [outer.path()], "personal": personal.path()}));
    let uri = "board://https://git.example.com/team/board/tasks/a";
    let (root, path) = by_uri(&named, uri, None).unwrap();
    assert_eq!(
        (real(&root), path.as_str()),
        (real(outer.workdir()), "/board/tasks/a")
    );
    let (unlisted, _xdg) = known(&json!({"personal": personal.path()}));
    let (root, path) = by_uri(&unlisted, uri, None).unwrap();
    assert_eq!(
        (real(&root), path.as_str()),
        (real(inner.workdir()), "/tasks/a")
    );
}

#[test]
fn two_mounted_clones_of_one_remote_are_ambiguous_and_one_mounted_twice_is_admitted() {
    let one = originated("https://github.com/acme/team-board.git");
    let two = originated("git@github.com:acme/team-board.git");
    let personal = mounting(&[&one, &two]);
    let (named, _xdg) = known(&json!({"personal": personal.path()}));
    let refusal = *by_uri(&named, URI, None).unwrap_err();
    assert_eq!(refusal.code.as_str(), "board::uri::ambiguous");
    for clone in [&one, &two] {
        assert!(
            refusal.message.contains(&clone.path()),
            "{}",
            refusal.message
        );
    }
    let twice = mounting(&[&one, &one]);
    let (named, _xdg) = known(&json!({"personal": twice.path()}));
    assert!(by_uri(&named, URI, None).is_ok());
    let both = by_uri(&named, URI, Some(personal.workdir())).unwrap_err();
    assert_eq!(both.code.as_str(), "board::uri::ambiguous");
}

#[test]
fn a_mount_that_is_not_a_board_and_a_missing_personal_board_are_passed_over() {
    let plain = Scenario::new();
    plain.commit(&[("x.json", "1")]);
    let remote = "[remote \"origin\"]\n\turl = git@github.com:acme/team-board.git\n";
    plain.write(".git/config", Item::Appended(remote));
    let personal = mounting(&[&plain]);
    let (named, _xdg) = known(&json!({"personal": personal.path()}));
    let lost = *by_uri(&named, URI, None).unwrap_err();
    assert_eq!(lost.code.as_str(), "board::uri::not_found");
    let gone = Scenario::new();
    let missing = gone.workdir().join("gone");
    let (named, _xdg) = known(&json!({"personal": missing}));
    let lost = *by_uri(&named, URI, None).unwrap_err();
    assert_eq!(lost.code.as_str(), "board::uri::not_found");
    assert!(
        lost.message.contains(&missing.display().to_string()),
        "{}",
        lost.message
    );
}
