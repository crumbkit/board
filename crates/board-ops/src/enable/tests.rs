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
        .plugin_enable(Some(&plugins), None, &notes())
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
        .plugin_enable(Some(&plugins), None, &notes())
        .unwrap();
    let again = open(&board)
        .plugin_enable(Some(&plugins), None, &notes())
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
        .plugin_enable(Some(&plugins), None, &notes())
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
        .plugin_enable(Some(&plugins), None, &notes())
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
        .plugin_enable(Some(&plugins), None, &notes())
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
fn a_schema_symlinked_to_a_different_one_refuses_the_enable_and_one_symlinked_to_an_equal_one_is_left()
 {
    let at = ".board/types/notes/note/v1.schema.json";
    let linked = |held: &str| {
        let (board, data, plugins) = installed(SCHEMA);
        board
            .write("schemas/note.json", Item::File(held))
            .write(at, Item::Symlink("../../../../schemas/note.json"));
        (board, data, plugins)
    };
    let (board, _data, plugins) = linked(r#"{"type": "object"}"#);
    let refused = open(&board)
        .plugin_enable(Some(&plugins), None, &notes())
        .unwrap_err();
    assert_eq!(refused.code(), Code::PutSymlink);
    assert!(
        open(&board).config().plugins.is_empty(),
        "nothing is written"
    );
    let (board, _data, plugins) = linked(SCHEMA);
    let report = open(&board)
        .plugin_enable(Some(&plugins), None, &notes())
        .unwrap();
    assert_eq!(report.put.writes.len(), 1, "only the config is written");
    let kept = std::fs::read_link(board.workdir().join(at)).unwrap();
    assert_eq!(kept, std::path::Path::new("../../../../schemas/note.json"));
}

#[test]
fn a_different_schema_in_the_board_is_replaced_and_reported() {
    let at = ".board/types/notes/note/v1.schema.json";
    // A file that is not JSON at all is another schema too.
    for other in [r#"{"type": "object"}"#, r#"{"type": "object",}"#] {
        let (board, _data, plugins) = installed(SCHEMA);
        board.write(at, Item::File(other));
        let report = open(&board)
            .plugin_enable(Some(&plugins), None, &notes())
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
        .plugin_enable(Some(&plugins), None, &notes())
        .unwrap();
    assert!(report.replaced.is_empty());
    assert_eq!(report.put.writes.len(), 1, "only the config is written");
    assert_eq!(read(&board, at), same.as_bytes());
}

#[test]
fn a_new_schema_is_written_but_not_reported_as_replaced() {
    let (board, _data, plugins) = installed(SCHEMA);
    let report = open(&board)
        .plugin_enable(Some(&plugins), None, &notes())
        .unwrap();
    assert!(report.replaced.is_empty());
}

#[test]
fn a_missing_or_unusable_shipped_schema_is_a_bad_manifest() {
    for schema in ["{", r#"{"type": 5}"#] {
        let (board, _data, plugins) = installed(schema);
        let refused = open(&board)
            .plugin_enable(Some(&plugins), None, &notes())
            .unwrap_err();
        assert_eq!(refused.code(), Code::PluginBadManifest, "{schema}");
    }
    let (board, data, plugins) = installed(SCHEMA);
    data.write(
        "plugins/notes/types/notes/note/v1.schema.json",
        Item::Deleted,
    );
    let refused = open(&board)
        .plugin_enable(Some(&plugins), None, &notes())
        .unwrap_err();
    assert_eq!(refused.code(), Code::PluginBadManifest);
    assert!(
        refused
            .to_string()
            .contains("types/notes/note/v1.schema.json")
    );
    let (board, _data, plugins) = installed(SCHEMA);
    assert!(
        open(&board)
            .plugin_enable(Some(&plugins), None, &notes())
            .is_ok()
    );
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
        .plugin_enable(Some(&plugins), None, &notes())
        .unwrap_err();
    assert!(matches!(refused, EnableError::Manifest { .. }));
    assert_eq!(refused.code(), Code::PluginBadManifest);
    let (board, _data, plugins) = installed(SCHEMA);
    assert!(
        open(&board)
            .plugin_enable(Some(&plugins), None, &notes())
            .is_ok()
    );
}

#[test]
fn a_plugin_that_is_not_installed_cannot_be_enabled() {
    let (board, _data, plugins) = installed(SCHEMA);
    let other: PluginName = "other".parse().unwrap();
    let refused = open(&board)
        .plugin_enable(Some(&plugins), None, &other)
        .unwrap_err();
    assert_eq!(refused.code(), Code::PluginNotInstalled);
    assert!(
        open(&board)
            .plugin_enable(Some(&plugins), None, &notes())
            .is_ok()
    );
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
    open(&board)
        .plugin_enable(Some(&plugins), None, &other)
        .unwrap();
    opened
        .plugin_enable(Some(&plugins), None, &notes())
        .unwrap();
    assert_eq!(open(&board).config().plugins, vec![other, notes()]);
}

#[test]
fn a_config_broken_since_the_board_was_opened_says_to_run_the_enable_again() {
    let (board, _data, plugins) = installed(SCHEMA);
    let opened = open(&board);
    board.write(".board/board.json", Item::File("{"));
    let refused = opened
        .plugin_enable(Some(&plugins), None, &notes())
        .unwrap_err();
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
    assert!(opened.plugin_enable(Some(&plugins), None, &notes()).is_ok());
}

/// Where the board holds the schema of `notes/old/v1`, which notes no
/// longer ships.
const OLD: &str = ".board/types/notes/old/v1.schema.json";

/// A board holding the schema of `notes/old/v1` and `records` records of it.
fn orphaned(config: &str, records: usize) -> (Scenario, Scenario, PathBuf) {
    let (board, data, plugins) = installed_with(config, MANIFEST, SCHEMA);
    board.write(OLD, Item::File(r#"{"type": "object"}"#));
    for at in 0..records {
        let path = format!("old/{at}.json");
        board.write(&path, Item::File(r#"{"$type": "notes/old/v1"}"#));
    }
    (board, data, plugins)
}

const UNCOMMITTED: &str = r#"{"$type": "board/config/v1", "commit_on_put": false}"#;

#[test]
fn a_schema_the_plugin_no_longer_ships_is_kept_while_a_record_has_its_type_and_removed_once_none_does()
 {
    let (board, _data, plugins) = orphaned(UNCOMMITTED, 2);
    let report = open(&board)
        .plugin_enable(Some(&plugins), None, &notes())
        .unwrap();
    assert!(report.removed.is_empty());
    let old: RecordType = "notes/old/v1".parse().unwrap();
    assert_eq!(
        report.kept,
        [Kept {
            schema: OLD.to_owned(),
            record_type: old,
            records: 2
        }]
    );
    assert!(board.workdir().join(OLD).exists());
    assert_eq!(open(&board).config().plugins, vec![notes()]);
    let note = ".board/types/notes/note/v1.schema.json";
    assert!(board.workdir().join(note).exists(), "the rest is written");

    board
        .write("old/0.json", Item::Deleted)
        .write("old/1.json", Item::Deleted);
    let report = open(&board)
        .plugin_enable(Some(&plugins), None, &notes())
        .unwrap();
    assert!(report.kept.is_empty());
    assert_eq!(report.removed, [OLD]);
    assert!(!board.workdir().join(OLD).exists());
}

#[test]
fn a_removed_schema_leaves_the_commit_with_the_rest_of_the_enable() {
    let config = r#"{"$type": "board/config/v1"}"#;
    let (board, _data, plugins) = orphaned(config, 0);
    board.write(
        ".git/config",
        Item::Appended("[user]\n\tname = Board Test\n\temail = board@example.com\n"),
    );
    board.commit(&[
        (OLD, r#"{"type": "object"}"#),
        (".board/board.json", config),
    ]);
    let report = open(&board)
        .plugin_enable(Some(&plugins), None, &notes())
        .unwrap();
    let head = board.head().unwrap();
    assert_eq!(Some(head.id), report.put.commit);
    assert!(!head.files.contains_key(OLD));
    assert!(
        head.files
            .contains_key(".board/types/notes/note/v1.schema.json")
    );
}

#[test]
fn a_schema_outside_the_plugins_namespace_or_symlinked_is_left_alone() {
    let (board, _data, plugins) = orphaned(UNCOMMITTED, 0);
    let other = ".board/types/other/old/v1.schema.json";
    let sibling = ".board/types/notes2/old/v1.schema.json";
    let linked = ".board/types/notes/linked/v1.schema.json";
    board
        .write(other, Item::File(r#"{"type": "object"}"#))
        .write(sibling, Item::File(r#"{"type": "object"}"#))
        .write(linked, Item::Symlink("../../other/old/v1.schema.json"));
    let report = open(&board)
        .plugin_enable(Some(&plugins), None, &notes())
        .unwrap();
    assert_eq!(report.removed, [OLD]);
    assert!(board.workdir().join(other).exists());
    assert!(board.workdir().join(sibling).exists());
    assert!(
        board.workdir().join(linked).exists(),
        "the link still resolves"
    );
}

#[test]
fn a_warm_index_finds_the_records_a_cold_one_does() {
    // Enable writes, so each run starts from its own board in the same state.
    let last = |warm: bool| {
        let (board, _data, plugins) = orphaned(UNCOMMITTED, 1);
        let library = crate::tests::Library::mounted(&board);
        let cache = tempfile::tempdir().unwrap();
        let enable = |cache| open(&board).plugin_enable(Some(&plugins), cache, &notes());
        assert_eq!(enable(Some(cache.path())).unwrap().kept.len(), 1, "cold");
        assert_eq!(enable(Some(cache.path())).unwrap().kept.len(), 1, "warm");
        library.moved(&board);
        board.write("old/0.json", Item::Deleted);
        let report = enable(warm.then_some(cache.path())).unwrap();
        (report.removed, report.kept, report.replaced)
    };
    let warm = last(true);
    assert_eq!(warm.0, [OLD], "the record gone");
    assert_eq!(warm, last(false));
}

#[test]
fn a_degraded_record_of_the_type_keeps_its_schema_and_a_valid_one_of_another_type_does_not() {
    let strict = r#"{"type": "object", "required": ["x"]}"#;
    let (board, _data, plugins) = orphaned(UNCOMMITTED, 0);
    board
        .write(OLD, Item::File(strict))
        .write("old/bad.json", Item::File(r#"{"$type": "notes/old/v1"}"#));
    let report = open(&board)
        .plugin_enable(Some(&plugins), None, &notes())
        .unwrap();
    assert_eq!(report.kept.len(), 1);
    let (board, _data, plugins) = orphaned(UNCOMMITTED, 0);
    board
        .write(OLD, Item::File(strict))
        .write("old/bad.json", Item::File(r#"{"$type": "notes/note/v1"}"#));
    let report = open(&board)
        .plugin_enable(Some(&plugins), None, &notes())
        .unwrap();
    assert!(report.kept.is_empty());
    assert_eq!(report.removed, [OLD]);
}

#[test]
fn an_enable_of_an_enabled_plugin_only_removes_what_it_no_longer_ships() {
    let config = r#"{"$type": "board/config/v1", "commit_on_put": false, "plugins": ["notes"]}"#;
    let (board, _data, plugins) = orphaned(config, 0);
    board.write(".board/types/notes/note/v1.schema.json", Item::File(SCHEMA));
    let report = open(&board)
        .plugin_enable(Some(&plugins), None, &notes())
        .unwrap();
    assert!(report.put.writes.is_empty());
    assert_eq!(report.removed, [OLD]);
    assert!(!board.workdir().join(OLD).exists());
}
