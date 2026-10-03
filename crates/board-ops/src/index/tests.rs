//! Building the index from real repositories, counting the work done.

use board_index::{BlobFact, Index, RefFact, ValidationFact};
use board_model::{Code, Kind, Role};
use board_scenario::{Item, Scenario};

use crate::index::Built;
use crate::tests::{PROJECT_SCHEMA, TASK_SCHEMA, board, open, task};

/// Builds `scenario` into the index in `cache`.
fn build(scenario: &Scenario, cache: &std::path::Path) -> Built {
    let board = open(scenario);
    let mut index = Index::open(Some(cache), board.repository().path()).unwrap();
    board.build_index(&mut index).unwrap()
}

/// The index of `scenario` in `cache`, built.
fn built(scenario: &Scenario, cache: &std::path::Path) -> Index {
    let board = open(scenario);
    let mut index = Index::open(Some(cache), board.repository().path()).unwrap();
    board.build_index(&mut index).unwrap();
    index
}

/// The board, plus two tasks with the same bytes and one invalid task.
fn tasks() -> Scenario {
    let scenario = board();
    scenario
        .write("tasks/a.json", Item::File(&task("")))
        .write("tasks/b.json", Item::File(&task("")))
        .write("tasks/bad.json", Item::File(r#"{"$type": "lr/task/v1"}"#))
        .write("notes.md", Item::File("not read"));
    scenario
}

#[test]
fn a_cold_build_parses_each_json_blob_once_and_validates_each_record_once() {
    let scenario = tasks();
    let cache = tempfile::tempdir().unwrap();
    // board.json, two schemas, the project, and two distinct task blobs;
    // notes.md is never read.
    assert_eq!(
        build(&scenario, cache.path()),
        Built {
            parsed: 6,
            validated: 3
        }
    );
}

#[test]
fn a_second_build_does_no_work() {
    let scenario = tasks();
    let cache = tempfile::tempdir().unwrap();
    build(&scenario, cache.path());
    assert_eq!(build(&scenario, cache.path()), Built::default());
}

#[test]
fn one_changed_record_parses_one_blob_and_validates_one_record() {
    let scenario = tasks();
    let cache = tempfile::tempdir().unwrap();
    build(&scenario, cache.path());
    scenario.write("tasks/a.json", Item::File(&task(r#", "see": "/x""#)));
    assert_eq!(
        build(&scenario, cache.path()),
        Built {
            parsed: 1,
            validated: 1
        }
    );
}

#[test]
fn a_changed_schema_revalidates_exactly_the_records_of_its_type() {
    let scenario = tasks();
    let cache = tempfile::tempdir().unwrap();
    build(&scenario, cache.path());
    scenario.write(
        ".board/types/lr/task/v1.schema.json",
        Item::File(r#"{"type": "object", "required": []}"#),
    );
    assert_eq!(
        build(&scenario, cache.path()),
        Built {
            parsed: 1,
            validated: 2
        }
    );
}

#[test]
fn the_index_holds_each_blobs_kind_type_and_code() {
    let scenario = tasks();
    let broken = r#"{"type": 7}"#;
    scenario
        .write(".board/types/lr/broken/v1.schema.json", Item::File(broken))
        .write("bad.json", Item::File("{"));
    let cache = tempfile::tempdir().unwrap();
    let mut index = built(&scenario, cache.path());
    let writer = index.write().unwrap();
    let fact = |text: &str, role| writer.blob(scenario.blob(text), role).unwrap();
    let fact_of = |kind, record_type: Option<&str>, code| {
        Some(BlobFact {
            kind,
            record_type: record_type.map(|text| text.parse().unwrap()),
            code,
        })
    };
    assert_eq!(
        fact(&task(""), Role::Json),
        fact_of(Kind::Record, Some("lr/task/v1"), None)
    );
    assert_eq!(fact(&task(""), Role::Schema), None, "only roles seen");
    assert_eq!(
        fact("{", Role::Json),
        fact_of(Kind::Json, None, Some(Code::JsonParse))
    );
    assert_eq!(
        fact(TASK_SCHEMA, Role::Schema),
        fact_of(Kind::Schema, None, None)
    );
    assert_eq!(
        fact(broken, Role::Schema),
        fact_of(Kind::Schema, None, Some(Code::SchemaInvalid)),
        "a schema's fact says whether it compiles"
    );
    assert_eq!(fact("not read", Role::Json), None);
}

#[test]
fn the_index_holds_each_validation_with_its_references_in_the_order_found() {
    let scenario = board();
    let good = task(r#", "see": "/x", "project": "/projects/p""#);
    let bad = r#"{"$type": "lr/task/v1"}"#;
    scenario
        .write("tasks/good.json", Item::File(&good))
        .write("tasks/bad.json", Item::File(bad));
    let cache = tempfile::tempdir().unwrap();
    let mut index = built(&scenario, cache.path());
    let writer = index.write().unwrap();
    let schema = scenario.blob(TASK_SCHEMA);
    assert_eq!(
        writer.validation(scenario.blob(&good), schema).unwrap(),
        Some(ValidationFact::Valid(vec![
            RefFact {
                pointer: "/project".to_owned(),
                target: "/projects/p".parse().unwrap(),
                accepts: Some(vec!["lr/project/v1".parse().unwrap()]),
            },
            RefFact {
                pointer: "/see".to_owned(),
                target: "/x".parse().unwrap(),
                accepts: None,
            },
        ]))
    );
    assert_eq!(
        writer.validation(scenario.blob(bad), schema).unwrap(),
        Some(ValidationFact::Invalid {
            code: Code::RecordInvalid,
            pointer: String::new(),
        })
    );
    assert_eq!(
        writer
            .validation(scenario.blob(&good), scenario.blob(PROJECT_SCHEMA))
            .unwrap(),
        None
    );
}
