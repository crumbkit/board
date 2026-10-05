use std::path::PathBuf;

use board_scenario::{Item, Scenario};

use super::*;
use crate::tests::open;

const NOTES: &str = r#"{
  "$type": "board/plugin-manifest/v1",
  "name": "notes",
  "version": "1.0.0",
  "description": "Notes.",
  "run": ["./run"],
  "metadata": {"area": "writing"},
  "capabilities": [
    {"name": "search", "description": "", "inputSchema": {"type": "object"}, "outputSchema": {"type": "object"}}
  ]
}"#;

const TASKS: &str = r#"{
  "$type": "board/plugin-manifest/v1",
  "name": "tasks",
  "version": "0.1.0",
  "description": "Open tasks.",
  "run": ["./run"],
  "metadata": {"area": "planning"},
  "capabilities": [
    {"name": "due", "description": "", "inputSchema": {"type": "object"}, "outputSchema": {"type": "object"}},
    {"name": "search", "description": "", "inputSchema": {"type": "object"}, "outputSchema": {"type": "object"},
     "_meta": {"area": "writing"}}
  ]
}"#;

/// A board enabling `enabled`, and a plugins directory holding notes, tasks
/// and a broken plugin.
fn installed(enabled: &str) -> (Scenario, Scenario, PathBuf) {
    let board = Scenario::new();
    board.write(
        ".board/board.json",
        Item::File(&format!(
            r#"{{"$type": "board/config/v1", "plugins": {enabled}}}"#
        )),
    );
    let data = Scenario::new();
    data.write("plugins/notes/plugin.json", Item::File(NOTES))
        .write("plugins/tasks/plugin.json", Item::File(TASKS))
        .write("plugins/broken/plugin.json", Item::File("{"));
    let plugins = data.workdir().join("plugins");
    (board, data, plugins)
}

fn listed(report: &PluginListReport) -> Vec<(&str, bool, Option<Code>)> {
    report
        .plugins
        .iter()
        .map(|plugin| (plugin.name.as_str(), plugin.enabled, plugin.code()))
        .collect()
}

#[test]
fn list_reports_every_installed_plugin_and_whether_the_board_enables_it() {
    let (board, _data, plugins) = installed(r#"["tasks"]"#);
    let report = open(&board).plugin_list(Some(&plugins)).unwrap();
    assert_eq!(
        listed(&report),
        [
            ("broken", false, Some(Code::PluginBadManifest)),
            ("notes", false, None),
            ("tasks", true, None),
        ]
    );
    assert!(report.degraded());
}

#[test]
fn an_enabled_plugin_that_is_not_installed_is_listed_with_its_code() {
    let (board, data, plugins) = installed(r#"["tasks", "ghost"]"#);
    data.write("plugins/broken", Item::Deleted);
    let report = open(&board).plugin_list(Some(&plugins)).unwrap();
    assert_eq!(
        listed(&report),
        [
            ("ghost", true, Some(Code::PluginNotInstalled)),
            ("notes", false, None),
            ("tasks", true, None),
        ]
    );
    data.write(
        "plugins/ghost/plugin.json",
        Item::File(&TASKS.replace("tasks", "ghost")),
    );
    let report = open(&board).plugin_list(Some(&plugins)).unwrap();
    assert_eq!(listed(&report)[0], ("ghost", true, None));
    assert!(!report.degraded());
}

#[test]
fn without_a_plugins_directory_only_enabled_plugins_are_listed() {
    let (board, _data, _plugins) = installed(r#"["tasks"]"#);
    let report = open(&board).plugin_list(None).unwrap();
    assert_eq!(
        listed(&report),
        [("tasks", true, Some(Code::PluginNotInstalled))]
    );
}

fn found(report: &PluginFindReport) -> Vec<String> {
    report
        .plugins
        .iter()
        .flat_map(|found| {
            found.manifest.capabilities.iter().map(|capability| {
                format!(
                    "{}.{} {}",
                    found.manifest.name, capability.name, found.enabled
                )
            })
        })
        .collect()
}

fn find(capability: Option<&str>, filters: &[&str]) -> Vec<String> {
    let (board, _data, plugins) = installed(r#"["tasks"]"#);
    let capability: Option<CapabilityName> = capability.map(|name| name.parse().unwrap());
    let filters: Vec<MetaFilter> = filters.iter().map(|text| text.parse().unwrap()).collect();
    let report = open(&board)
        .plugin_find(Some(&plugins), capability.as_ref(), &filters)
        .unwrap();
    found(&report)
}

#[test]
fn find_filters_by_capability_and_skips_broken_plugins() {
    assert_eq!(
        find(Some("search"), &[]),
        ["notes.search false", "tasks.search true"]
    );
    assert_eq!(find(Some("due"), &[]), ["tasks.due true"]);
    assert!(find(Some("late"), &[]).is_empty());
    assert_eq!(
        find(None, &[]),
        ["notes.search false", "tasks.due true", "tasks.search true"]
    );
}

#[test]
fn find_matches_metadata_on_the_capability_then_the_plugin() {
    assert_eq!(
        find(None, &["area=writing"]),
        ["notes.search false", "tasks.search true"]
    );
    assert_eq!(find(None, &["area=planning"]), ["tasks.due true"]);
    assert_eq!(find(Some("due"), &["area=writing"]), Vec::<String>::new());
}

/// A board enabling `enabled`, and a plugins directory holding notes, which
/// ships the schema of `notes/note/v1`.
fn shipping(enabled: &str) -> (Scenario, Scenario, PathBuf) {
    let (board, data, plugins) = installed(enabled);
    data.write("plugins/tasks", Item::Deleted)
        .write("plugins/broken", Item::Deleted)
        .write(
            "plugins/notes/plugin.json",
            Item::File(&NOTES.replace(r#""run""#, r#""types": ["notes/note/v1"], "run""#)),
        )
        .write(
            "plugins/notes/types/notes/note/v1.schema.json",
            Item::File(r#"{"type": "object"}"#),
        );
    (board, data, plugins)
}

fn schemas(report: &PluginListReport) -> Vec<(String, String, Code)> {
    report.plugins[0]
        .schemas
        .iter()
        .map(|problem| {
            (
                problem.record_type.to_string(),
                problem.path.clone(),
                problem.code,
            )
        })
        .collect()
}

#[test]
fn an_enabled_plugin_whose_schema_is_missing_is_degraded_and_one_holding_it_is_not() {
    let (board, _data, plugins) = shipping(r#"["notes"]"#);
    let report = open(&board).plugin_list(Some(&plugins)).unwrap();
    assert_eq!(
        listed(&report),
        [("notes", true, Some(Code::PluginSchemaMissing))]
    );
    assert_eq!(
        schemas(&report),
        [(
            "notes/note/v1".to_owned(),
            ".board/types/notes/note/v1.schema.json".to_owned(),
            Code::PluginSchemaMissing
        )]
    );
    assert!(report.degraded());

    board.write(
        ".board/types/notes/note/v1.schema.json",
        Item::File(r#"{"type":"object"}"#),
    );
    let report = open(&board).plugin_list(Some(&plugins)).unwrap();
    assert_eq!(listed(&report), [("notes", true, None)]);
    assert!(!report.degraded());
}

#[test]
fn an_enabled_plugin_whose_schema_differs_is_degraded_and_one_holding_the_same_is_not() {
    let (board, _data, plugins) = shipping(r#"["notes"]"#);
    board.write(
        ".board/types/notes/note/v1.schema.json",
        Item::File(r#"{"type": "array"}"#),
    );
    let report = open(&board).plugin_list(Some(&plugins)).unwrap();
    assert_eq!(
        listed(&report),
        [("notes", true, Some(Code::PluginSchemaDiffers))]
    );
    assert_eq!(
        schemas(&report),
        [(
            "notes/note/v1".to_owned(),
            ".board/types/notes/note/v1.schema.json".to_owned(),
            Code::PluginSchemaDiffers
        )]
    );
    board.write(
        ".board/types/notes/note/v1.schema.json",
        Item::File(r#"{"type": "object"}"#),
    );
    let report = open(&board).plugin_list(Some(&plugins)).unwrap();
    assert_eq!(listed(&report), [("notes", true, None)]);
}

#[test]
fn a_plugin_the_board_does_not_enable_needs_no_schema() {
    let (board, _data, plugins) = shipping("[]");
    let report = open(&board).plugin_list(Some(&plugins)).unwrap();
    assert_eq!(listed(&report), [("notes", false, None)]);
    assert!(report.plugins[0].schemas.is_empty());
}

#[test]
fn a_plugin_shipping_no_usable_schema_is_a_bad_manifest_offering_nothing_and_a_usable_one_is_offered()
 {
    for enabled in ["[]", r#"["notes"]"#] {
        let (board, data, plugins) = shipping(enabled);
        board.write(
            ".board/types/notes/note/v1.schema.json",
            Item::File(r#"{"type": "object"}"#),
        );
        data.write(
            "plugins/notes/types/notes/note/v1.schema.json",
            Item::File(r#"{"type": 3}"#),
        );
        let board = open(&board);
        let report = board.plugin_list(Some(&plugins)).unwrap();
        assert_eq!(
            listed(&report),
            [("notes", enabled != "[]", Some(Code::PluginBadManifest))]
        );
        assert!(report.plugins[0].schemas.is_empty());
        let found = board.plugin_find(Some(&plugins), None, &[]).unwrap();
        assert!(found.plugins.is_empty(), "{enabled}");

        data.write(
            "plugins/notes/types/notes/note/v1.schema.json",
            Item::File(r#"{"type": "object"}"#),
        );
        let report = board.plugin_list(Some(&plugins)).unwrap();
        assert_eq!(listed(&report), [("notes", enabled != "[]", None)]);
        let found = board.plugin_find(Some(&plugins), None, &[]).unwrap();
        assert_eq!(found.plugins.len(), 1, "{enabled}");
    }
}

#[test]
fn a_schema_the_board_cannot_read_is_unreadable_and_a_readable_one_is_clean() {
    let (board, _data, plugins) = shipping(r#"["notes"]"#);
    board.write(".board/types/notes/note/v1.schema.json/x", Item::File("{}"));
    let report = open(&board).plugin_list(Some(&plugins)).unwrap();
    assert_eq!(
        listed(&report),
        [("notes", true, Some(Code::RepoUnreadable))]
    );
    board
        .write(".board/types/notes/note/v1.schema.json", Item::Deleted)
        .write(
            ".board/types/notes/note/v1.schema.json",
            Item::File(r#"{"type": "object"}"#),
        );
    let report = open(&board).plugin_list(Some(&plugins)).unwrap();
    assert_eq!(listed(&report), [("notes", true, None)]);
}
