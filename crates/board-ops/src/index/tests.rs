//! Building the index from real repositories, counting the work done.

use board_index::{BlobFact, Index, RefFact, ValidationFact};
use board_model::{Code, Kind, Role};
use board_scenario::{Item, Scenario};

use crate::index::Refreshed;
use crate::tests::{PROJECT_SCHEMA, TASK_SCHEMA, board, open, task};

/// Refreshes the index of `scenario` in `cache`.
fn build(scenario: &Scenario, cache: &std::path::Path) -> Refreshed {
    let board = open(scenario);
    let mut index = Index::open(Some(cache), board.repository().path()).unwrap();
    board.refresh_index(&mut index).unwrap()
}

/// The index of `scenario` in `cache`, built.
fn built(scenario: &Scenario, cache: &std::path::Path) -> Index {
    let board = open(scenario);
    let mut index = Index::open(Some(cache), board.repository().path()).unwrap();
    board.refresh_index(&mut index).unwrap();
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
fn a_cold_refresh_hashes_every_file_and_parses_each_json_blob_once() {
    let scenario = tasks();
    let cache = tempfile::tempdir().unwrap();
    // Eight files are hashed; board.json, two schemas, the project, and two
    // distinct task blobs are parsed; notes.md is never read.
    assert_eq!(
        build(&scenario, cache.path()),
        Refreshed {
            hashed: 8,
            parsed: 6,
            validated: 3,
            ..Refreshed::default()
        }
    );
}

#[test]
fn a_second_refresh_does_no_work() {
    let scenario = tasks();
    let cache = tempfile::tempdir().unwrap();
    build(&scenario, cache.path());
    assert_eq!(build(&scenario, cache.path()), Refreshed::default());
}

#[test]
fn one_changed_record_hashes_one_file_parses_one_blob_and_validates_one_record() {
    let scenario = tasks();
    let cache = tempfile::tempdir().unwrap();
    build(&scenario, cache.path());
    scenario.write("tasks/a.json", Item::File(&task(r#", "see": "/x""#)));
    assert_eq!(
        build(&scenario, cache.path()),
        Refreshed {
            hashed: 1,
            parsed: 1,
            validated: 1,
            ..Refreshed::default()
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
        Refreshed {
            hashed: 1,
            parsed: 1,
            validated: 2,
            ..Refreshed::default()
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

#[test]
fn a_file_modified_in_the_second_the_last_refresh_began_is_hashed_again() {
    let scenario = tasks();
    scenario.write("fresh.md", Item::Fresh("just saved"));
    let cache = tempfile::tempdir().unwrap();
    build(&scenario, cache.path());
    assert_eq!(
        build(&scenario, cache.path()),
        Refreshed {
            hashed: 1,
            ..Refreshed::default()
        },
        "its metadata matches, but it could have changed unseen"
    );
}

#[test]
fn a_file_modified_before_the_last_refresh_began_is_trusted() {
    let scenario = tasks();
    scenario.write("fresh.md", Item::File("saved an hour ago"));
    let cache = tempfile::tempdir().unwrap();
    build(&scenario, cache.path());
    assert_eq!(build(&scenario, cache.path()), Refreshed::default());
}

#[test]
fn a_deleted_file_leaves_the_index_and_a_new_one_is_hashed() {
    let scenario = tasks();
    let cache = tempfile::tempdir().unwrap();
    build(&scenario, cache.path());
    scenario.write("notes.md", Item::Deleted);
    scenario.write("more.md", Item::File("new"));
    assert_eq!(
        build(&scenario, cache.path()),
        Refreshed {
            hashed: 1,
            ..Refreshed::default()
        }
    );
    let mut index = built(&scenario, cache.path());
    let paths: Vec<String> = {
        let writer = index.write().unwrap();
        let mut paths: Vec<String> = writer.worktree().unwrap().into_keys().collect();
        paths.sort();
        paths
    };
    assert!(paths.contains(&"more.md".to_owned()));
    assert!(!paths.contains(&"notes.md".to_owned()));
}

#[test]
fn the_index_keeps_each_entrys_address_code_blob_and_metadata() {
    let scenario = tasks();
    scenario.write("bad\tname.md", Item::File(""));
    let cache = tempfile::tempdir().unwrap();
    let mut index = built(&scenario, cache.path());
    let writer = index.write().unwrap();
    let rows = writer.worktree().unwrap();
    let a = &rows["tasks/a.json"];
    assert_eq!(a.logical, Some("/tasks/a".parse().unwrap()));
    assert_eq!(a.code, None);
    assert_eq!(a.blob, Some(scenario.blob(&task(""))));
    assert_eq!(a.stat.unwrap().size, task("").len() as u64);
    let malformed = &rows["bad\tname.md"];
    assert_eq!(malformed.logical, None);
    assert_eq!(malformed.code, Some(Code::PathMalformed));
}

#[test]
fn head_s_trees_are_listed_once_and_after_a_commit_only_the_changed_ones() {
    let scenario = Scenario::new();
    scenario.commit(&[
        (".board/board.json", r#"{"$type": "board/config/v1"}"#),
        ("a/x.md", "1"),
        ("b/y.md", "1"),
    ]);
    scenario.write(
        ".board/board.json",
        Item::File(r#"{"$type": "board/config/v1"}"#),
    );
    let cache = tempfile::tempdir().unwrap();
    assert_eq!(
        build(&scenario, cache.path()).trees,
        4,
        "root, .board, a, b"
    );
    assert_eq!(build(&scenario, cache.path()).trees, 0);
    scenario.commit(&[
        (".board/board.json", r#"{"$type": "board/config/v1"}"#),
        ("a/x.md", "2"),
        ("b/y.md", "1"),
    ]);
    assert_eq!(
        build(&scenario, cache.path()).trees,
        2,
        "the new root and a"
    );
}
