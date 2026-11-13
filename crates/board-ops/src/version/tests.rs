//! Reading versions and build dates, and comparing them with a board's
//! `min_board`.

use board_model::Code;
use board_scenario::{Item, Scenario};
use board_tree::LOCK_WAIT;

use super::{Built, Version};
use crate::Expect;
use crate::put::{Removing, Unmatched};
use crate::tests::{board, open, task};

/// 2026-11-13T00:00:00Z.
const NOV_13: i64 = 1_794_528_000;
const DAY: i64 = 86_400;

fn version(text: &str) -> Version {
    text.parse().unwrap()
}

/// The test board with `min_board` set to the JSON `min_board`, or not set
/// when `None`.
fn gated(min_board: Option<&str>) -> Scenario {
    let scenario = board();
    let config = match min_board {
        Some(json) => format!(r#"{{"$type": "board/config/v1", "min_board": {json}}}"#),
        None => r#"{"$type": "board/config/v1"}"#.to_owned(),
    };
    scenario.write(".board/board.json", Item::File(&config));
    scenario.identified();
    scenario
}

fn running(scenario: &Scenario, version: &str, built: Option<i64>) -> crate::Board {
    let mut board = open(scenario);
    board.running(version, built);
    board
}

fn put_as(scenario: &Scenario, version: &str, built: Option<i64>) -> Result<(), Code> {
    let path = "/tasks/a".parse().unwrap();
    let record = task(r#", "project": "/projects/p""#);
    running(scenario, version, built)
        .put(&path, Expect::Any, record.as_bytes())
        .map(|_| ())
        .map_err(|error| error.code())
}

fn sync_as(scenario: &Scenario, version: &str, built: Option<i64>) -> Result<(), Code> {
    running(scenario, version, built)
        .sync(false, board_tree::GIT_DEADLINE)
        .map(|_| ())
        .map_err(|error| error.code())
}

fn rm_as(scenario: &Scenario, version: &str, built: Option<i64>) -> Result<(), Code> {
    let path = "/projects/p".parse().unwrap();
    running(scenario, version, built)
        .remove_waiting(
            &path,
            Removing::File(Expect::Any),
            Unmatched::Refuse,
            None,
            LOCK_WAIT,
        )
        .map(|_| ())
        .map_err(|error| error.code())
}

fn batch_as(scenario: &Scenario, version: &str, built: Option<i64>) -> Result<(), Code> {
    let record = task(r#", "project": "/projects/p""#);
    let line = format!(r#"{{"path": "/tasks/a", "record": {record}}}"#) + "\n";
    running(scenario, version, built)
        .put_batch(&mut line.as_bytes())
        .map(|_| ())
        .map_err(|error| error.code())
}

const REFUSED: Result<(), Code> = Err(Code::ConfigBoardTooOld);

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
fn a_build_date_is_an_rfc_3339_timestamp_that_prints_in_utc() {
    let built: Built = "2026-11-13T10:00:00+10:00".parse().unwrap();
    assert_eq!(built, Built::from_seconds(NOV_13));
    assert_eq!(built.to_string(), "2026-11-13T00:00:00Z");
    assert!("2026-11-13".parse::<Built>().is_err());
}

#[test]
fn an_older_version_is_refused_a_put_and_an_equal_or_newer_one_is_admitted() {
    let scenario = gated(Some(r#""1.2.0""#));
    assert_eq!(put_as(&scenario, "1.1.9", None), REFUSED);
    assert_eq!(put_as(&scenario, "1.2.0+abc", None), Ok(()));
    assert_eq!(put_as(&scenario, "1.3.0", None), Ok(()));
}

#[test]
fn an_older_version_is_refused_a_sync_and_an_equal_or_newer_one_is_not() {
    let scenario = gated(Some(r#""1.2.0""#));
    assert_eq!(sync_as(&scenario, "1.1.9", None), REFUSED);
    // The board has no remote, so a sync that is admitted stops later, for
    // another reason.
    assert_ne!(sync_as(&scenario, "1.2.0", None), REFUSED);
    assert_ne!(sync_as(&scenario, "1.3.0", None), REFUSED);
}

#[test]
fn an_older_version_is_refused_an_rm_and_an_equal_one_is_admitted() {
    let scenario = gated(Some(r#""1.2.0""#));
    assert_eq!(rm_as(&scenario, "1.1.9", None), REFUSED);
    assert_eq!(rm_as(&scenario, "1.2.0", None), Ok(()));
}

#[test]
fn an_older_version_is_refused_a_batch_and_an_equal_one_is_admitted() {
    let scenario = gated(Some(r#""1.2.0""#));
    assert_eq!(batch_as(&scenario, "1.1.9", None), REFUSED);
    assert_eq!(batch_as(&scenario, "1.2.0", None), Ok(()));
}

#[test]
fn an_older_version_still_reads() {
    let scenario = gated(Some(r#""1.2.0""#));
    let board = running(&scenario, "1.1.9", None);
    assert!(board.get(&"/projects/p".parse().unwrap()).is_ok());
}

#[test]
fn a_board_without_min_board_admits_every_version_and_build() {
    let scenario = gated(None);
    assert_eq!(put_as(&scenario, "0.0.1", Some(0)), Ok(()));
}

#[test]
fn a_source_build_numbered_0_0_0_is_refused_by_a_version_and_admitted_by_0_0_0() {
    let scenario = gated(Some(r#""0.1.0""#));
    assert_eq!(put_as(&scenario, "0.0.0+45aafa9", None), REFUSED);
    let scenario = gated(Some(r#""0.0.0""#));
    assert_eq!(put_as(&scenario, "0.0.0+45aafa9", None), Ok(()));
}

#[test]
fn a_timestamp_refuses_a_build_dated_before_it_and_admits_one_at_or_after_it() {
    let scenario = gated(Some(r#""2026-11-13T00:00:00Z""#));
    assert_eq!(put_as(&scenario, "0.0.0", Some(NOV_13 - 1)), REFUSED);
    assert_eq!(put_as(&scenario, "0.0.0", Some(NOV_13)), Ok(()));
    assert_eq!(put_as(&scenario, "0.0.0", Some(NOV_13 + DAY)), Ok(()));
}

#[test]
fn a_timestamp_admits_a_build_with_no_date_because_the_version_rule_is_all_it_has() {
    let scenario = gated(Some(r#""2026-11-13T00:00:00Z""#));
    assert_eq!(put_as(&scenario, "0.0.0", None), Ok(()));
}

#[test]
fn an_object_refuses_a_build_older_on_either_count() {
    let scenario = gated(Some(
        r#"{"version": "1.2.0", "built": "2026-11-13T00:00:00Z"}"#,
    ));
    // Old on the version only, on the date only, and on both.
    assert_eq!(put_as(&scenario, "1.1.0", Some(NOV_13 + DAY)), REFUSED);
    assert_eq!(put_as(&scenario, "1.2.0", Some(NOV_13 - DAY)), REFUSED);
    assert_eq!(put_as(&scenario, "1.1.0", Some(NOV_13 - DAY)), REFUSED);
    // Not old on either, and old on the date only but with no date known.
    assert_eq!(put_as(&scenario, "1.2.0", Some(NOV_13)), Ok(()));
    assert_eq!(put_as(&scenario, "1.2.0", None), Ok(()));
    assert_eq!(put_as(&scenario, "1.1.0", None), REFUSED);
}

#[test]
fn an_object_may_name_only_one_count() {
    let scenario = gated(Some(r#"{"built": "2026-11-13T00:00:00Z"}"#));
    assert_eq!(put_as(&scenario, "0.0.0", Some(NOV_13 - 1)), REFUSED);
    let scenario = gated(Some(r#"{"version": "1.2.0"}"#));
    assert_eq!(put_as(&scenario, "1.1.0", Some(NOV_13)), REFUSED);
}

#[test]
fn a_min_board_that_names_nothing_or_is_not_understood_makes_the_config_invalid() {
    for json in [
        r#""soon""#,
        "{}",
        r#"{"version": "x"}"#,
        r#"{"built": "yesterday"}"#,
        r#"{"since": "1.0.0"}"#,
        "3",
    ] {
        let scenario = gated(Some(json));
        let error = crate::Board::open(scenario.workdir(), None).unwrap_err();
        assert_eq!(error.code(), Code::ConfigInvalid, "{json}");
    }
}

#[test]
fn the_refusal_names_what_the_board_needs_and_what_is_running() {
    let scenario = gated(Some(
        r#"{"version": "1.2.0", "built": "2026-11-13T00:00:00Z"}"#,
    ));
    let path = "/tasks/a".parse().unwrap();
    let error = running(&scenario, "1.1.0", None)
        .put(&path, Expect::Any, b"{}")
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "this board needs a Board with version 1.2.0 or newer and a build dated 2026-11-13T00:00:00Z or later to write it, and this is Board 1.1.0 (no build date); reads still work"
    );
}
