//! Opening boards and checking them, against real repositories.

use board_model::{Code, Kind, LogicalPath};
use board_scenario::{Item, Scenario};
use board_tree::Status;

use crate::{Board, CheckReport};

const CONFIG: &str = r#"{"$type": "board/config/v1"}"#;

const TASK_SCHEMA: &str = r#"{
  "type": "object",
  "required": ["title"],
  "properties": {
    "title": {"type": "string"},
    "project": {"type": "string", "format": "board-ref", "x-board-type": "lr/project/v1"},
    "see": {"type": "string", "format": "board-ref"}
  }
}"#;

const PROJECT_SCHEMA: &str = r#"{"type": "object"}"#;

/// A board with the task and project types and one project.
fn board() -> Scenario {
    let scenario = Scenario::new();
    scenario
        .write(".board/board.json", Item::File(CONFIG))
        .write(
            ".board/types/lr/task/v1.schema.json",
            Item::File(TASK_SCHEMA),
        )
        .write(
            ".board/types/lr/project/v1.schema.json",
            Item::File(PROJECT_SCHEMA),
        )
        .write(
            "projects/p.json",
            Item::File(r#"{"$type": "lr/project/v1"}"#),
        );
    scenario
}

fn open(scenario: &Scenario) -> Board {
    Board::open(scenario.workdir(), None).unwrap()
}

fn check(scenario: &Scenario) -> CheckReport {
    open(scenario).check(None).unwrap()
}

/// The status `check` gives the entry at `path`.
fn status(scenario: &Scenario, path: &str) -> Status {
    check(scenario)
        .entries
        .into_iter()
        .find(|entry| entry.path == path)
        .unwrap_or_else(|| panic!("{path} is not an entry"))
        .status
}

fn task(fields: &str) -> String {
    format!(r#"{{"$type": "lr/task/v1", "title": "t"{fields}}}"#)
}

#[test]
fn a_board_is_found_from_any_directory_inside_it() {
    let scenario = board();
    scenario.write("work/deep/x.md", Item::File(""));
    let from_inside = Board::open(&scenario.workdir().join("work/deep"), None).unwrap();
    assert_eq!(
        from_inside.repository().path(),
        open(&scenario).repository().path()
    );
    assert!(from_inside.config().commit_on_put);
    assert!(from_inside.config().plugins.is_empty());
}

#[test]
fn without_board_json_at_the_repository_root_there_is_no_board() {
    let plain = Scenario::new();
    plain.write("x.json", Item::File("{}"));
    assert_eq!(
        Board::open(plain.workdir(), None).unwrap_err().code(),
        Code::ConfigNotFound
    );
    let nested = Scenario::new();
    nested.write("sub/.board/board.json", Item::File(CONFIG));
    let error = Board::open(&nested.workdir().join("sub"), None).unwrap_err();
    assert_eq!(error.code(), Code::ConfigNotFound);
    assert!(Board::open(board().workdir(), None).is_ok());
}

#[test]
fn a_board_json_board_cannot_read_is_invalid() {
    for bad in [
        r#"{"$type": "board/config/v1", "colour": "red"}"#,
        r#"{"$type": "board/config/v2"}"#,
        r#"{"$type": "board/config/v1", "plugins": [1]}"#,
        "{",
    ] {
        let scenario = Scenario::new();
        scenario.write(".board/board.json", Item::File(bad));
        let error = Board::open(scenario.workdir(), None).unwrap_err();
        assert_eq!(error.code(), Code::ConfigInvalid, "{bad}");
    }
    let scenario = Scenario::new();
    scenario.write(
        ".board/board.json",
        Item::File(r#"{"$type": "board/config/v1", "commit_on_put": false, "plugins": ["tasks"]}"#),
    );
    let board = open(&scenario);
    assert!(!board.config().commit_on_put);
    assert_eq!(board.config().plugins, ["tasks"]);
}

#[test]
fn a_clean_board_checks_ok_with_every_kind_reported() {
    let scenario = board();
    let to_p = r#"{"$type": "board/mount/v1", "path": "/projects/p"}"#;
    scenario
        .write(
            "work/t.json",
            Item::File(&task(r#", "project": "/projects/p""#)),
        )
        .write("work/l.json", Item::File(to_p))
        .write("work/data.json", Item::File("[1, 2]"))
        .write("work/notes.md", Item::File("# notes"));
    let report = check(&scenario);
    assert!(!report.degraded(), "{report:#?}");
    let kinds: Vec<(&str, Kind)> = report
        .entries
        .iter()
        .map(|entry| (entry.path.as_str(), entry.kind))
        .collect();
    assert_eq!(
        kinds,
        [
            (".board/board.json", Kind::Record),
            (".board/types/lr/project/v1.schema.json", Kind::Schema),
            (".board/types/lr/task/v1.schema.json", Kind::Schema),
            ("projects/p.json", Kind::Record),
            ("work/data.json", Kind::Json),
            ("work/l.json", Kind::Pointer),
            ("work/notes.md", Kind::File),
            ("work/t.json", Kind::Record),
        ]
    );
}

#[test]
fn a_record_whose_type_has_no_schema_is_unknown() {
    let scenario = board();
    scenario
        .write("a.json", Item::File(&task("")))
        .write("b.json", Item::File(r#"{"$type": "lr/note/v1"}"#));
    assert_eq!(status(&scenario, "a.json"), Status::Ok);
    assert_eq!(
        status(&scenario, "b.json"),
        Status::Degraded(Code::RecordUnknownType)
    );
}

#[test]
fn a_schema_that_does_not_compile_degrades_itself_and_its_records() {
    let scenario = board();
    scenario
        .write(
            ".board/types/lr/note/v1.schema.json",
            Item::File(r#"{"type": 5}"#),
        )
        .write("n.json", Item::File(r#"{"$type": "lr/note/v1"}"#))
        .write("t.json", Item::File(&task("")));
    let schema = ".board/types/lr/note/v1.schema.json";
    assert_eq!(
        status(&scenario, schema),
        Status::Degraded(Code::SchemaInvalid)
    );
    assert_eq!(
        status(&scenario, "n.json"),
        Status::Degraded(Code::SchemaInvalid)
    );
    assert_eq!(status(&scenario, "t.json"), Status::Ok);
}

#[test]
fn a_record_its_schema_refuses_is_invalid() {
    let scenario = board();
    scenario.write("ok.json", Item::File(&task(""))).write(
        "bad.json",
        Item::File(r#"{"$type": "lr/task/v1", "title": 1}"#),
    );
    assert_eq!(status(&scenario, "ok.json"), Status::Ok);
    assert_eq!(
        status(&scenario, "bad.json"),
        Status::Degraded(Code::RecordInvalid)
    );
}

#[test]
fn references_are_checked_for_shape_target_and_type() {
    let scenario = board();
    scenario
        .write(
            "ok.json",
            Item::File(&task(r#", "project": "/projects/p""#)),
        )
        .write(
            "malformed.json",
            Item::File(&task(r#", "project": "projects/p""#)),
        )
        .write(
            "dangling.json",
            Item::File(&task(r#", "project": "/projects/q""#)),
        )
        .write("wrong.json", Item::File(&task(r#", "project": "/ok""#)))
        .write("untyped.json", Item::File(&task(r#", "see": "/ok""#)))
        .write(
            "untyped-dangling.json",
            Item::File(&task(r#", "see": "/nowhere""#)),
        );
    assert_eq!(status(&scenario, "ok.json"), Status::Ok);
    assert_eq!(
        status(&scenario, "malformed.json"),
        Status::Degraded(Code::RefMalformed)
    );
    assert_eq!(
        status(&scenario, "dangling.json"),
        Status::Degraded(Code::RefDangling)
    );
    assert_eq!(
        status(&scenario, "wrong.json"),
        Status::Degraded(Code::RefWrongType)
    );
    assert_eq!(status(&scenario, "untyped.json"), Status::Ok);
    assert_eq!(
        status(&scenario, "untyped-dangling.json"),
        Status::Degraded(Code::RefDangling)
    );
}

#[test]
fn a_reference_to_something_other_than_a_record_is_the_wrong_type() {
    let scenario = board();
    scenario.write("projects/notes.md", Item::File("")).write(
        "t.json",
        Item::File(&task(r#", "project": "/projects/notes.md""#)),
    );
    assert_eq!(
        status(&scenario, "t.json"),
        Status::Degraded(Code::RefWrongType)
    );
}

#[test]
fn a_pointer_whose_target_is_missing_is_dangling_in_the_walk() {
    let scenario = board();
    scenario
        .write(
            "good.json",
            Item::File(r#"{"$type": "board/mount/v1", "path": "/projects/p"}"#),
        )
        .write(
            "bad.json",
            Item::File(r#"{"$type": "board/mount/v1", "path": "/projects/q"}"#),
        );
    assert_eq!(status(&scenario, "good.json"), Status::Ok);
    assert_eq!(
        status(&scenario, "bad.json"),
        Status::Degraded(Code::RefDangling)
    );
}

#[test]
fn parse_problems_and_path_problems_come_first() {
    // Every name here is one a case-insensitive file system can hold; case
    // collisions are covered on commits in board-tree, on every platform.
    let scenario = board();
    scenario
        .write("dup.json", Item::File(r#"{"a": 1, "a": 2}"#))
        .write(".json", Item::File("{"))
        .write("t.json", Item::File(&task("")));
    assert_eq!(
        status(&scenario, "dup.json"),
        Status::Degraded(Code::JsonDuplicateKey)
    );
    assert_eq!(
        status(&scenario, ".json"),
        Status::Degraded(Code::PathMalformed)
    );
    assert_eq!(status(&scenario, "t.json"), Status::Ok);
}

#[test]
fn a_scope_checks_only_entries_at_or_beneath_it() {
    let scenario = board();
    scenario
        .write("work/t.json", Item::File(&task("")))
        .write("workshop/t.json", Item::File(&task("")));
    let scope: LogicalPath = "/work".parse().unwrap();
    let report = open(&scenario).check(Some(&scope)).unwrap();
    let paths: Vec<&str> = report
        .entries
        .iter()
        .map(|entry| entry.path.as_str())
        .collect();
    assert_eq!(paths, ["work/t.json"]);
    let missing: LogicalPath = "/nowhere".parse().unwrap();
    let error = open(&scenario).check(Some(&missing)).unwrap_err();
    assert_eq!(error.code(), Code::PathNotFound);
}

#[test]
fn check_fails_when_any_entry_is_degraded() {
    let scenario = board();
    assert!(!check(&scenario).degraded());
    scenario.write("bad.json", Item::File("{"));
    assert!(check(&scenario).degraded());
}
