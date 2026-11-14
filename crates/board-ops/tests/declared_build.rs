//! A binary declares the build it is once, and every board opened after that
//! gates its writes as that build, including a board that no command opens
//! through the binary's own open path, such as the personal board an answer
//! or a held effect writes (docs/architecture/writes/minimum.md). This file
//! is a process of its own, so the declaration reaches only these tests.
#![cfg(test)]

use board_model::Code;
use board_ops::{Board, Expect};
use board_scenario::{Item, Scenario};

/// 2026-11-13T00:00:00Z.
const NOV_13: i64 = 1_794_528_000;

fn gated(min_board: &str) -> Scenario {
    let scenario = Scenario::board();
    scenario.write(
        ".board/board.json",
        Item::File(&format!(
            r#"{{"$type": "board/config/v1", "min_board": {min_board}}}"#
        )),
    );
    scenario.write(
        ".board/types/lr/note/v1.schema.json",
        Item::File(r#"{"type": "object"}"#),
    );
    scenario.identified();
    scenario
}

#[test]
fn a_board_opened_plainly_is_gated_as_the_declared_build() {
    board_ops::declare_build("0.0.0+abc123", Some(NOV_13 - 1));
    let scenario = gated(r#""2026-11-13T00:00:00Z""#);
    let board = Board::open(scenario.workdir(), None).unwrap();
    let error = board
        .put(
            &"/note".parse().unwrap(),
            Expect::Any,
            br#"{"$type": "lr/note/v1"}"#,
        )
        .unwrap_err();
    assert_eq!(error.code(), Code::ConfigBoardTooOld);
    assert!(error.to_string().contains("Board 0.0.0+abc123"), "{error}");

    let admitting = gated(r#""2026-11-12T00:00:00Z""#);
    let board = Board::open(admitting.workdir(), None).unwrap();
    board
        .put(
            &"/note".parse().unwrap(),
            Expect::Any,
            br#"{"$type": "lr/note/v1"}"#,
        )
        .unwrap();
}
