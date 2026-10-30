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
    assert_eq!(
        found[1].origin,
        Origin::Directory(plugins(&scenario).join("tasks"))
    );
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

#[test]
fn a_plugins_directory_that_cannot_be_listed_is_refused_as_not_installed() {
    let scenario = Scenario::new();
    scenario.write("plugins", Item::File(""));
    let error = installed(&plugins(&scenario)).unwrap_err();
    assert_eq!(error.code(), Code::PluginNotInstalled);
    assert!(error.to_string().contains("plugins"), "{error}");
    scenario
        .write("plugins", Item::Deleted)
        .write("plugins/tasks/plugin.json", Item::File(TASKS));
    assert_eq!(names(&installed(&plugins(&scenario)).unwrap()), ["tasks"]);
}

const NOTES: &str = r#"{
  "$type": "board/plugin-manifest/v1",
  "name": "notes",
  "version": "1.0.0",
  "description": "Notes.",
  "run": ["./run"],
  "types": ["notes/note/v1"]
}"#;

const SHIPPED: &str = "plugins/notes/types/notes/note/v1.schema.json";

#[test]
fn a_type_whose_shipped_schema_is_missing_or_unusable_is_a_bad_manifest_naming_the_file() {
    let scenario = Scenario::new();
    scenario.write("plugins/notes/plugin.json", Item::File(NOTES));
    let shipped = plugins(&scenario).join("notes/types/notes/note/v1.schema.json");
    for (schema, reason) in [
        (None, "is missing or cannot be read"),
        (Some("{"), "is not strict JSON"),
        (Some(r#"{"type": 5}"#), "does not compile"),
    ] {
        if let Some(schema) = schema {
            scenario.write(SHIPPED, Item::File(schema));
        }
        let found = installed(&plugins(&scenario)).unwrap();
        let error = found[0].manifest.as_ref().unwrap_err();
        assert_eq!(error.code(), Code::PluginBadManifest);
        let message = error.to_string();
        assert!(
            message.contains(&shipped.display().to_string()),
            "{message}"
        );
        assert!(message.contains("notes/note/v1"), "{message}");
        assert!(message.contains(reason), "{message}");
        assert!(found[0].schemas.is_empty());
    }
    scenario.write(SHIPPED, Item::File(r#"{"type": "object"}"#));
    let found = installed(&plugins(&scenario)).unwrap();
    assert!(found[0].manifest.is_ok());
    assert_eq!(
        found[0].schemas,
        [(
            "notes/note/v1".parse().unwrap(),
            serde_json::json!({"type": "object"})
        )]
    );
}

#[test]
fn a_built_in_plugin_must_ship_a_compiling_schema_for_each_declared_type() {
    let manifest = || {
        let mut value: Value = serde_json::from_str(TASKS).unwrap();
        value["types"] = serde_json::json!(["tasks/task/v1"]);
        Manifest::built_in(value).unwrap()
    };
    let shipped = |schema: Value| vec![("tasks/task/v1".parse().unwrap(), schema)];
    let admitted = built_in(
        3,
        manifest(),
        &shipped(serde_json::json!({"type": "object"})),
    );
    assert_eq!(admitted.name, "tasks");
    assert_eq!(
        admitted.origin,
        Origin::BuiltIn {
            index: 3,
            shadows: None
        }
    );
    assert_eq!(
        admitted.schemas,
        shipped(serde_json::json!({"type": "object"}))
    );
    let missing = built_in(3, manifest(), &[]);
    assert_eq!(missing.name, "tasks");
    assert!(matches!(
        missing.manifest.unwrap_err(),
        ManifestError::ShippedMissing { .. }
    ));
    let broken = built_in(3, manifest(), &shipped(serde_json::json!({"type": 5})));
    assert!(matches!(
        broken.manifest.unwrap_err(),
        ManifestError::ShippedSchema { .. }
    ));
    assert!(broken.schemas.is_empty());
}
