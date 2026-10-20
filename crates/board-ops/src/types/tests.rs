//! Adding a type to a board from its schema.

use board_scenario::{Item, Scenario};

use super::*;
use crate::put::Expect;
use crate::tests::open;

const SCHEMA: &str = r#"{"type": "object", "required": ["title"]}"#;

/// The schema as `type_add` writes it: canonical.
const WRITTEN: &[u8] = b"{\n  \"required\": [\n    \"title\"\n  ],\n  \"type\": \"object\"\n}\n";

const AT: &str = ".board/types/notes/note/v1.schema.json";

/// A board with commits off.
fn board() -> Scenario {
    let board = Scenario::new();
    board.write(
        ".board/board.json",
        Item::File(r#"{"$type": "board/config/v1", "commit_on_put": false}"#),
    );
    board
}

fn note(version: u32) -> RecordType {
    format!("notes/note/v{version}").parse().unwrap()
}

fn read(board: &Scenario, path: &str) -> Vec<u8> {
    std::fs::read(board.workdir().join(path)).unwrap()
}

#[test]
fn adding_a_type_writes_its_schema_in_canonical_form_and_a_record_of_it_then_puts() {
    let board = board();
    let report = open(&board).type_add(&note(1), SCHEMA.as_bytes()).unwrap();
    assert_eq!(report.writes.len(), 1);
    assert_eq!(
        report.writes[0].display.as_str(),
        "/.board/types/notes/note/v1.schema"
    );
    assert_eq!(report.writes[0].previous, None);
    assert_eq!(read(&board, AT), WRITTEN);
    let record = br#"{"$type": "notes/note/v1", "title": "a"}"#;
    let path = "/notes/a".parse().unwrap();
    assert!(open(&board).put(&path, Expect::Absent, record).is_ok());
}

#[test]
fn a_board_that_commits_on_put_commits_the_added_type() {
    let board = Scenario::new();
    board
        .write(
            ".board/board.json",
            Item::File(r#"{"$type": "board/config/v1"}"#),
        )
        .write(
            ".git/config",
            Item::Appended("[user]\n\tname = Board Test\n\temail = board@example.com\n"),
        );
    let report = open(&board).type_add(&note(1), SCHEMA.as_bytes()).unwrap();
    let head = board.head().unwrap();
    assert_eq!(Some(head.id), report.commit);
    assert_eq!(head.message, "Add the type notes/note/v1");
    assert_eq!(head.files.keys().collect::<Vec<_>>(), [AT]);
}

#[test]
fn the_same_schema_again_writes_nothing_and_a_different_one_is_refused() {
    let board = board();
    open(&board).type_add(&note(1), SCHEMA.as_bytes()).unwrap();
    let same = r#"{"required": ["title"], "type": "object"}"#;
    let again = open(&board).type_add(&note(1), same.as_bytes()).unwrap();
    assert!(again.writes.is_empty());
    assert_eq!(again.commit, None);
    let other = r#"{"type": "object", "required": ["title", "body"]}"#;
    let refused = open(&board)
        .type_add(&note(1), other.as_bytes())
        .unwrap_err();
    assert_eq!(refused.code(), Code::SchemaDiffers);
    assert!(refused.to_string().contains(AT), "{refused}");
    assert_eq!(
        refused.refusal().fields.get("schema"),
        Some(&serde_json::json!(AT))
    );
    assert_eq!(read(&board, AT), WRITTEN, "nothing is written");
    // The changed schema is admitted as the next version.
    assert!(open(&board).type_add(&note(2), other.as_bytes()).is_ok());
}

#[test]
fn a_schema_that_does_not_compile_is_refused_and_one_that_does_is_admitted() {
    let board = board();
    let refused = open(&board)
        .type_add(&note(1), br#"{"type": 5}"#)
        .unwrap_err();
    assert_eq!(refused.code(), Code::SchemaInvalid);
    assert!(!board.workdir().join(AT).exists(), "nothing is written");
    assert!(
        open(&board)
            .type_add(&note(1), br#"{"type": "object"}"#)
            .is_ok()
    );
}

#[test]
fn a_schema_that_is_not_json_is_refused_with_the_parse_code() {
    let board = board();
    let refused = open(&board).type_add(&note(1), b"{").unwrap_err();
    assert_eq!(refused.code(), Code::JsonParse);
    assert!(open(&board).type_add(&note(1), b"{}").is_ok());
}

#[test]
fn a_type_in_the_board_namespace_is_refused_and_one_in_another_namespace_is_admitted() {
    let board = board();
    let reserved: RecordType = "board/note/v1".parse().unwrap();
    let refused = open(&board)
        .type_add(&reserved, SCHEMA.as_bytes())
        .unwrap_err();
    assert_eq!(refused.code(), Code::SchemaInvalid);
    assert!(refused.to_string().contains("board/note/v1"), "{refused}");
    let refusal = refused.refusal();
    assert_eq!(
        refusal.fields.get("type"),
        Some(&serde_json::json!("board/note/v1"))
    );
    let hint = refusal.hint.unwrap();
    assert!(hint.contains("a namespace of your own"), "{hint}");
    assert!(!board.workdir().join(".board/types/board").exists());
    assert!(open(&board).type_add(&note(1), SCHEMA.as_bytes()).is_ok());
}

#[test]
fn an_equal_schema_left_uncommitted_is_committed_and_a_committed_one_is_left_as_it_is() {
    let board = Scenario::new();
    board
        .write(
            ".git/config",
            Item::Appended("[user]\n\tname = Board Test\n\temail = board@example.com\n"),
        )
        .write(
            ".board/board.json",
            Item::File(r#"{"$type": "board/config/v1"}"#),
        )
        .write(AT, Item::File(std::str::from_utf8(WRITTEN).unwrap()));
    let committed = open(&board).type_add(&note(1), SCHEMA.as_bytes()).unwrap();
    assert!(committed.writes.is_empty(), "{committed:?}");
    let head = board.head().unwrap();
    assert_eq!(committed.commit, Some(head.id));
    assert!(head.files.contains_key(AT), "{:?}", head.files.keys());
    let again = open(&board).type_add(&note(1), SCHEMA.as_bytes()).unwrap();
    assert!(again.writes.is_empty());
    assert_eq!(again.commit, None);
}

#[test]
fn a_board_schema_that_is_not_json_is_refused_with_its_parse_code_and_a_json_one_is_compared() {
    let board = board();
    board.write(AT, Item::File("{"));
    let refused = open(&board)
        .type_add(&note(1), SCHEMA.as_bytes())
        .unwrap_err();
    assert_eq!(refused.code(), Code::JsonParse);
    assert_eq!(
        refused.refusal().fields.get("schema"),
        Some(&serde_json::json!(AT))
    );
    assert_eq!(read(&board, AT), b"{", "nothing is written");
    board.write(AT, Item::File(std::str::from_utf8(WRITTEN).unwrap()));
    assert!(open(&board).type_add(&note(1), SCHEMA.as_bytes()).is_ok());
}

#[test]
fn a_type_with_a_part_over_255_bytes_is_refused_with_a_hint_to_shorten_it() {
    let board = board();
    let named =
        |bytes: usize| -> RecordType { format!("notes/{}/v1", "k".repeat(bytes)).parse().unwrap() };
    let refused = open(&board)
        .type_add(&named(256), SCHEMA.as_bytes())
        .unwrap_err();
    assert_eq!(refused.code(), Code::PathMalformed);
    let hint = refused.refusal().hint.unwrap();
    assert!(hint.contains("shorten the type's name"), "{hint}");
    assert!(
        open(&board)
            .type_add(&named(255), SCHEMA.as_bytes())
            .is_ok()
    );
}

#[test]
fn an_equal_schema_in_another_layout_is_kept_as_it_is_committed_or_not() {
    let layout = r#"{ "required" : ["title"], "type" : "object" }"#;
    let config = r#"{"$type": "board/config/v1"}"#;
    let board = Scenario::new();
    board
        .write(
            ".git/config",
            Item::Appended("[user]\n\tname = Board Test\n\temail = board@example.com\n"),
        )
        .write(".board/board.json", Item::File(config))
        .write(AT, Item::File(layout));
    let committed = board.commit(&[(".board/board.json", config), (AT, layout)]);
    let kept = open(&board).type_add(&note(1), SCHEMA.as_bytes()).unwrap();
    assert!(kept.writes.is_empty(), "{kept:?}");
    assert_eq!(kept.commit, None);
    assert_eq!(board.head().unwrap().id, committed, "nothing is committed");
    assert_eq!(read(&board, AT), layout.as_bytes(), "the layout is kept");
    // Left uncommitted, the same file is committed as it is.
    let uncommitted = AT.replace("v1", "v2");
    board.write(&uncommitted, Item::File(layout));
    let added = open(&board).type_add(&note(2), SCHEMA.as_bytes()).unwrap();
    let head = board.head().unwrap();
    assert_eq!(added.commit, Some(head.id));
    assert_eq!(head.files[&uncommitted], layout.as_bytes());
    assert_eq!(read(&board, &uncommitted), layout.as_bytes());
}
