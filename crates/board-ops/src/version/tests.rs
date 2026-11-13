//! Reading versions, and comparing them with a board's `min_board`.

use board_model::Code;
use board_scenario::{Item, Scenario};

use super::Version;
use crate::Expect;
use crate::tests::{board, open, task};

fn version(text: &str) -> Version {
    text.parse().unwrap()
}

/// The test board with `min_board` set, or not set when `None`.
fn gated(min_board: Option<&str>) -> Scenario {
    let scenario = board();
    let config = match min_board {
        Some(minimum) => format!(r#"{{"$type": "board/config/v1", "min_board": "{minimum}"}}"#),
        None => r#"{"$type": "board/config/v1"}"#.to_owned(),
    };
    scenario.write(".board/board.json", Item::File(&config));
    scenario
}

fn put_as(scenario: &Scenario, running: &str) -> Result<(), Code> {
    let mut board = open(scenario);
    board.running(running);
    let path = "/tasks/a".parse().unwrap();
    let record = task(r#", "project": "/projects/p""#);
    board
        .put(&path, Expect::Any, record.as_bytes())
        .map(|_| ())
        .map_err(|error| error.code())
}

fn sync_as(scenario: &Scenario, running: &str) -> Result<(), Code> {
    let mut board = open(scenario);
    board.running(running);
    board
        .sync(false, board_tree::GIT_DEADLINE)
        .map(|_| ())
        .map_err(|error| error.code())
}

#[test]
fn a_version_reads_major_minor_patch_and_ignores_a_suffix() {
    assert_eq!(version("1.2.3").to_string(), "1.2.3");
    assert_eq!(version("0.0.0+45aafa9"), version("0.0.0"));
    assert_eq!(version("1.2.3-rc1"), version("1.2.3"));
    assert!(version("1.10.0") > version("1.9.9"));
    for bad in ["", "1", "1.2", "1.2.3.4", "1.x.3", "v1.2.3", "1.2.-3"] {
        assert!(bad.parse::<Version>().is_err(), "{bad}");
    }
}

#[test]
fn an_older_board_is_refused_a_put_and_an_equal_or_newer_one_is_admitted() {
    let scenario = gated(Some("1.2.0"));
    assert_eq!(put_as(&scenario, "1.1.9"), Err(Code::ConfigBoardTooOld));
    assert_eq!(put_as(&scenario, "1.2.0+abc"), Ok(()));
    assert_eq!(put_as(&scenario, "1.3.0"), Ok(()));
}

#[test]
fn an_older_board_is_refused_a_sync_and_an_equal_or_newer_one_is_not() {
    let scenario = gated(Some("1.2.0"));
    assert_eq!(sync_as(&scenario, "1.1.9"), Err(Code::ConfigBoardTooOld));
    // The board has no remote, so a sync that is admitted stops later, for
    // another reason.
    assert_ne!(sync_as(&scenario, "1.2.0"), Err(Code::ConfigBoardTooOld));
    assert_ne!(sync_as(&scenario, "1.3.0"), Err(Code::ConfigBoardTooOld));
}

#[test]
fn an_older_board_still_reads() {
    let scenario = gated(Some("1.2.0"));
    let mut board = open(&scenario);
    board.running("1.1.9");
    assert!(board.get(&"/projects/p".parse().unwrap()).is_ok());
}

#[test]
fn a_board_without_min_board_admits_every_version() {
    let scenario = gated(None);
    assert_eq!(put_as(&scenario, "0.0.1"), Ok(()));
}

#[test]
fn an_unreleased_build_is_admitted_whatever_min_board_says() {
    let scenario = gated(Some("9.0.0"));
    assert_eq!(put_as(&scenario, "0.0.0+45aafa9"), Ok(()));
}

#[test]
fn a_min_board_that_is_no_version_makes_the_config_invalid() {
    let scenario = gated(Some("soon"));
    let error = crate::Board::open(scenario.workdir(), None).unwrap_err();
    assert_eq!(error.code(), Code::ConfigInvalid);
}
