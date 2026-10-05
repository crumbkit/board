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
