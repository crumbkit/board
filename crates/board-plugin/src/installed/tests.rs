use board_scenario::{Item, Scenario};

use super::*;

const TASKS: &str = r#"{
  "$type": "board/plugin-manifest/v1",
  "name": "tasks",
  "version": "0.1.0",
  "description": "Open tasks.",
  "run": ["./run"]
}"#;

fn plugins(scenario: &Scenario) -> PathBuf {
    scenario.workdir().join("plugins")
}

fn names(found: &[Installed]) -> Vec<&str> {
    found
        .iter()
        .map(|installed| installed.name.as_str())
        .collect()
}

#[test]
fn a_missing_plugins_directory_holds_no_plugins() {
    let scenario = Scenario::new();
    assert!(installed(&plugins(&scenario)).unwrap().is_empty());
}

#[test]
fn every_entry_is_found_in_name_order() {
    let scenario = Scenario::new();
    scenario
        .write("plugins/tasks/plugin.json", Item::File(TASKS))
        .write("plugins/notes/plugin.json", Item::File("{}"));
    let found = installed(&plugins(&scenario)).unwrap();
    assert_eq!(names(&found), ["notes", "tasks"]);
    assert_eq!(found[1].directory, plugins(&scenario).join("tasks"));
    assert!(found[1].manifest.is_ok());
    assert_eq!(
        found[0].manifest.as_ref().unwrap_err().code(),
        Code::PluginBadManifest
    );
}

#[test]
fn an_entry_without_a_manifest_is_a_bad_manifest() {
    let scenario = Scenario::new();
    scenario
        .write("plugins/empty/run", Item::File(""))
        .write("plugins/stray", Item::File(""))
        .write("plugins/tasks/plugin.json", Item::File(TASKS));
    let found = installed(&plugins(&scenario)).unwrap();
    assert_eq!(names(&found), ["empty", "stray", "tasks"]);
    for broken in &found[..2] {
        assert!(matches!(broken.manifest, Err(ManifestError::Missing)));
    }
    assert!(found[2].manifest.is_ok());
}

#[test]
fn a_plugin_may_be_installed_as_a_symlink_to_its_checkout() {
    let scenario = Scenario::new();
    scenario
        .write("checkout/tasks/plugin.json", Item::File(TASKS))
        .write("plugins/tasks", Item::Symlink("../checkout/tasks"))
        .write("plugins/gone", Item::Symlink("../checkout/gone"));
    let found = installed(&plugins(&scenario)).unwrap();
    assert_eq!(names(&found), ["gone", "tasks"]);
    assert!(matches!(found[0].manifest, Err(ManifestError::Missing)));
    assert!(found[1].manifest.is_ok());
}
