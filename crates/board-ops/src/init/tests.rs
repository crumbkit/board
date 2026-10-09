//! Making a git repository's working tree a board.

use board_model::Code;
use board_scenario::{Item, Scenario};

use crate::board::Board;
use crate::init::init;

const CONFIG: &[u8] = b"{\n  \"$type\": \"board/config/v1\"\n}\n";

fn config(scenario: &Scenario) -> Option<Vec<u8>> {
    std::fs::read(scenario.workdir().join(".board/board.json")).ok()
}

#[test]
fn init_writes_a_minimal_board_json_that_opens_as_a_board() {
    let scenario = Scenario::new();
    assert!(Board::open(scenario.workdir(), None).is_err());
    let report = init(scenario.workdir()).unwrap();
    assert!(report.created);
    assert_eq!(report.root, scenario.workdir().canonicalize().unwrap());
    assert_eq!(config(&scenario).as_deref(), Some(CONFIG));
    assert!(Board::open(scenario.workdir(), None).is_ok());
}

#[test]
fn init_from_a_subdirectory_writes_at_the_repository_root() {
    let scenario = Scenario::new();
    scenario.write("docs/a.md", Item::File("# a"));
    let report = init(&scenario.workdir().join("docs")).unwrap();
    assert!(report.created);
    assert_eq!(config(&scenario).as_deref(), Some(CONFIG));
}

#[test]
fn init_leaves_an_existing_board_json_as_it_is() {
    let scenario = Scenario::new();
    let existing = r#"{"$type": "board/config/v1", "commit_on_put": false}"#;
    scenario.write(".board/board.json", Item::File(existing));
    let report = init(scenario.workdir()).unwrap();
    assert!(!report.created);
    assert_eq!(config(&scenario).as_deref(), Some(existing.as_bytes()));
}

#[test]
fn init_refuses_an_invalid_board_json_and_admits_a_valid_one() {
    let scenario = Scenario::new();
    scenario.write(".board/board.json", Item::File("{}"));
    assert_eq!(
        init(scenario.workdir()).unwrap_err().code(),
        Code::ConfigInvalid
    );
    scenario.write(
        ".board/board.json",
        Item::File(r#"{"$type": "board/config/v1"}"#),
    );
    assert!(!init(scenario.workdir()).unwrap().created);
}

#[test]
fn init_refuses_a_directory_where_board_json_goes_and_admits_a_file() {
    let scenario = Scenario::new();
    scenario.write(".board/board.json/a.md", Item::File(""));
    assert_eq!(
        init(scenario.workdir()).unwrap_err().code(),
        Code::PutFailed
    );
    scenario.write(".board/board.json", Item::Deleted);
    assert!(init(scenario.workdir()).unwrap().created);
}

#[test]
fn init_does_not_write_through_a_symlinked_board_directory() {
    let scenario = Scenario::new();
    scenario.write("shared/a.md", Item::File(""));
    scenario.write(".board", Item::Symlink("shared"));
    let refused = init(scenario.workdir()).unwrap_err();
    assert_eq!(refused.code(), Code::PutSymlink);
    assert!(refused.to_string().ends_with(".board is a symlink"));
    assert!(!scenario.workdir().join("shared/board.json").exists());
    scenario.write(".board", Item::Deleted);
    assert!(init(scenario.workdir()).unwrap().created);
}

#[test]
fn init_outside_a_git_repository_is_refused_as_no_board() {
    let outside = tempfile::tempdir().unwrap();
    assert_eq!(
        init(outside.path()).unwrap_err().code(),
        Code::ConfigNotFound
    );
    let inside = Scenario::new();
    assert!(init(inside.workdir()).is_ok());
}
