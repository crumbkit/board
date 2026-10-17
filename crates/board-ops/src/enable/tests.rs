//! Enabling a plugin and the schemas it ships.

use std::path::PathBuf;

use board_scenario::{Item, Scenario};

use super::*;
use crate::tests::open;

const MANIFEST: &str = r#"{"$type": "board/plugin-manifest/v1", "name": "notes", "version": "1.0.0",
  "description": "Notes.", "run": ["./run"], "types": ["notes/note/v1"]}"#;

const SCHEMA: &str = r#"{"type": "object", "required": ["title"]}"#;

/// The schema as `enable` writes it: canonical.
const WRITTEN: &[u8] = b"{\n  \"required\": [\n    \"title\"\n  ],\n  \"type\": \"object\"\n}\n";

/// A board with commits off, and a plugins directory holding notes, which
/// ships the schema `schema`.
fn installed(schema: &str) -> (Scenario, Scenario, PathBuf) {
    installed_with(
        r#"{"$type": "board/config/v1", "commit_on_put": false}"#,
        MANIFEST,
        schema,
    )
}

/// A board configured by `config`, and a plugins directory holding notes,
/// with the manifest `manifest` and the schema `schema`.
fn installed_with(config: &str, manifest: &str, schema: &str) -> (Scenario, Scenario, PathBuf) {
    let board = Scenario::new();
    board.write(".board/board.json", Item::File(config));
    let data = Scenario::new();
    data.write("plugins/notes/plugin.json", Item::File(manifest))
        .write(
            "plugins/notes/types/notes/note/v1.schema.json",
            Item::File(schema),
        );
    let plugins = data.workdir().join("plugins");
    (board, data, plugins)
}

fn notes() -> PluginName {
    "notes".parse().unwrap()
}

fn read(board: &Scenario, path: &str) -> Vec<u8> {
    std::fs::read(board.workdir().join(path)).unwrap()
}

#[test]
fn enable_writes_the_shipped_schema_and_names_the_plugin_in_the_config() {
    let (board, _data, plugins) = installed(SCHEMA);
    let report = open(&board)
        .plugin_enable(Some(&plugins), &notes())
        .unwrap();
    assert_eq!(report.put.writes.len(), 2);
    assert_eq!(
        read(&board, ".board/types/notes/note/v1.schema.json"),
        WRITTEN
    );
    assert_eq!(open(&board).config().plugins, vec![notes()]);
    assert!(
        !open(&board).config().commit_on_put,
        "other fields are kept"
    );
}

#[test]
fn enabling_twice_writes_nothing_the_second_time() {
    let (board, _data, plugins) = installed(SCHEMA);
    open(&board)
        .plugin_enable(Some(&plugins), &notes())
        .unwrap();
    let again = open(&board)
        .plugin_enable(Some(&plugins), &notes())
        .unwrap();
    assert!(again.put.writes.is_empty());
    assert_eq!(again.put.commit, None);
}

#[test]
fn an_enabled_plugin_whose_schema_is_missing_gets_only_the_schema() {
    let (board, _data, plugins) = installed_with(
        r#"{"$type": "board/config/v1", "commit_on_put": false, "plugins": ["notes"]}"#,
        MANIFEST,
        SCHEMA,
    );
    let config = read(&board, ".board/board.json");
    let report = open(&board)
        .plugin_enable(Some(&plugins), &notes())
        .unwrap();
    assert_eq!(report.put.writes.len(), 1);
    assert_eq!(
        read(&board, ".board/types/notes/note/v1.schema.json"),
        WRITTEN
    );
    assert_eq!(read(&board, ".board/board.json"), config);
}

#[test]
fn a_plugin_that_ships_no_types_gets_only_the_config() {
    let manifest = MANIFEST.replace(r#", "types": ["notes/note/v1"]"#, "");
    let (board, _data, plugins) = installed_with(
        r#"{"$type": "board/config/v1", "commit_on_put": false}"#,
        &manifest,
        SCHEMA,
    );
    let report = open(&board)
        .plugin_enable(Some(&plugins), &notes())
        .unwrap();
    assert_eq!(report.put.writes.len(), 1);
    assert!(!board.workdir().join(".board/types").exists());
    assert_eq!(open(&board).config().plugins, vec![notes()]);
}

#[test]
fn a_board_that_commits_on_put_commits_the_enable_once() {
    let (board, _data, plugins) =
        installed_with(r#"{"$type": "board/config/v1"}"#, MANIFEST, SCHEMA);
    board.write(
        ".git/config",
        Item::Appended("[user]\n\tname = Board Test\n\temail = board@example.com\n"),
    );
    let report = open(&board)
        .plugin_enable(Some(&plugins), &notes())
        .unwrap();
    let head = board.head().unwrap();
    assert_eq!(Some(head.id), report.put.commit);
    assert_eq!(head.message, "Enable the notes plugin");
    assert_eq!(head.parents, []);
    assert_eq!(
        head.files.keys().collect::<Vec<_>>(),
        [
            ".board/board.json",
            ".board/types/notes/note/v1.schema.json"
        ]
    );
}

#[test]
fn a_symlinked_schema_refuses_the_enable_and_a_regular_copy_is_kept() {
    let (board, _data, plugins) = installed(SCHEMA);
    let at = ".board/types/notes/note/v1.schema.json";
    board
        .write("schemas/note.json", Item::File(SCHEMA))
        .write(at, Item::Symlink("../../../../schemas/note.json"));
    let refused = open(&board)
        .plugin_enable(Some(&plugins), &notes())
        .unwrap_err();
    assert_eq!(refused.code(), Code::PutSymlink);
    assert!(
        open(&board).config().plugins.is_empty(),
        "nothing is written"
    );
    board.write(at, Item::Deleted).write(at, Item::File(SCHEMA));
    let report = open(&board)
        .plugin_enable(Some(&plugins), &notes())
        .unwrap();
    assert_eq!(report.put.writes.len(), 1, "only the config is written");
    assert_eq!(read(&board, at), SCHEMA.as_bytes());
}

#[test]
fn a_different_schema_in_the_board_is_replaced_and_reported() {
    let at = ".board/types/notes/note/v1.schema.json";
    // A file that is not JSON at all is another schema too.
    for other in [r#"{"type": "object"}"#, r#"{"type": "object",}"#] {
        let (board, _data, plugins) = installed(SCHEMA);
        board.write(at, Item::File(other));
        let report = open(&board)
            .plugin_enable(Some(&plugins), &notes())
            .unwrap();
        assert_eq!(report.replaced, [at], "{other}");
        assert_eq!(report.put.writes.len(), 2, "{other}");
        assert_eq!(read(&board, at), WRITTEN, "{other}");
        assert_eq!(open(&board).config().plugins, vec![notes()]);
    }
}

#[test]
fn the_same_schema_in_another_layout_is_left_alone_and_not_reported() {
    let (board, _data, plugins) = installed(SCHEMA);
    let at = ".board/types/notes/note/v1.schema.json";
    let same = r#"{"required": ["title"], "type": "object"}"#;
    board.write(at, Item::File(same));
    let report = open(&board)
        .plugin_enable(Some(&plugins), &notes())
        .unwrap();
    assert!(report.replaced.is_empty());
    assert_eq!(report.put.writes.len(), 1, "only the config is written");
    assert_eq!(read(&board, at), same.as_bytes());
}

#[test]
fn a_new_schema_is_written_but_not_reported_as_replaced() {
    let (board, _data, plugins) = installed(SCHEMA);
    let report = open(&board)
        .plugin_enable(Some(&plugins), &notes())
        .unwrap();
    assert!(report.replaced.is_empty());
}

#[test]
fn a_missing_or_unusable_shipped_schema_is_a_bad_manifest() {
    for schema in ["{", r#"{"type": 5}"#] {
        let (board, _data, plugins) = installed(schema);
        let refused = open(&board)
            .plugin_enable(Some(&plugins), &notes())
            .unwrap_err();
        assert_eq!(refused.code(), Code::PluginBadManifest, "{schema}");
    }
    let (board, data, plugins) = installed(SCHEMA);
    data.write(
        "plugins/notes/types/notes/note/v1.schema.json",
        Item::Deleted,
    );
    let refused = open(&board)
        .plugin_enable(Some(&plugins), &notes())
        .unwrap_err();
    assert_eq!(refused.code(), Code::PluginBadManifest);
    assert!(
        refused
            .to_string()
            .contains("types/notes/note/v1.schema.json")
    );
    let (board, _data, plugins) = installed(SCHEMA);
    assert!(open(&board).plugin_enable(Some(&plugins), &notes()).is_ok());
}

#[test]
fn a_plugin_whose_manifest_is_refused_cannot_be_enabled() {
    let broken = MANIFEST.replace(r#""run": ["./run"]"#, r#""run": []"#);
    let (board, _data, plugins) = installed_with(
        r#"{"$type": "board/config/v1", "commit_on_put": false}"#,
        &broken,
        SCHEMA,
    );
    let refused = open(&board)
        .plugin_enable(Some(&plugins), &notes())
        .unwrap_err();
    assert!(matches!(refused, EnableError::Manifest { .. }));
    assert_eq!(refused.code(), Code::PluginBadManifest);
    let (board, _data, plugins) = installed(SCHEMA);
    assert!(open(&board).plugin_enable(Some(&plugins), &notes()).is_ok());
}

#[test]
fn a_plugin_that_is_not_installed_cannot_be_enabled() {
    let (board, _data, plugins) = installed(SCHEMA);
    let other: PluginName = "other".parse().unwrap();
    let refused = open(&board)
        .plugin_enable(Some(&plugins), &other)
        .unwrap_err();
    assert_eq!(refused.code(), Code::PluginNotInstalled);
    assert!(open(&board).plugin_enable(Some(&plugins), &notes()).is_ok());
}

#[test]
fn a_plugin_enabled_since_the_board_was_opened_is_kept() {
    let (board, data, plugins) = installed(SCHEMA);
    let opened = open(&board);
    data.write(
        "plugins/other/plugin.json",
        Item::File(
            &MANIFEST
                .replace(r#""notes""#, r#""other""#)
                .replace(r#", "types": ["notes/note/v1"]"#, ""),
        ),
    );
    let other: PluginName = "other".parse().unwrap();
    open(&board).plugin_enable(Some(&plugins), &other).unwrap();
    opened.plugin_enable(Some(&plugins), &notes()).unwrap();
    assert_eq!(open(&board).config().plugins, vec![other, notes()]);
}

#[test]
fn a_config_broken_since_the_board_was_opened_says_to_run_the_enable_again() {
    let (board, _data, plugins) = installed(SCHEMA);
    let opened = open(&board);
    board.write(".board/board.json", Item::File("{"));
    let refused = opened.plugin_enable(Some(&plugins), &notes()).unwrap_err();
    assert_eq!(refused.code(), Code::PutStale);
    assert!(refused.to_string().contains("run the enable again"));
    assert!(
        !board.workdir().join(".board/types").exists(),
        "nothing is written"
    );
    board.write(
        ".board/board.json",
        Item::File(r#"{"$type": "board/config/v1", "commit_on_put": false}"#),
    );
    assert!(opened.plugin_enable(Some(&plugins), &notes()).is_ok());
}
