//! A built-in plugin's moved prefixes: a reference under either prefix
//! resolves while the board holds its target under the other
//! (docs/plugins/calling/commands.md, Renamed plugins).

use board_model::{CapabilityName, Code, RecordType};
use board_plugin::Manifest;
use board_scenario::{Item, Scenario};
use board_tree::Status;
use serde_json::Value;

use super::{MANIFEST, board, open};
use crate::host::{Host, Plugin, PluginError};
use crate::{Board, Plugins};

/// A built-in plugin whose records moved between the prefixes it holds.
struct Renamed(&'static [(&'static str, &'static str)]);

impl Plugin for Renamed {
    fn manifest(&self) -> Manifest {
        let manifest = MANIFEST.replace(r#""run": ["./run"], "#, "");
        Manifest::built_in(serde_json::from_str(&manifest).unwrap()).unwrap()
    }

    fn schemas(&self) -> Vec<(RecordType, Value)> {
        Vec::new()
    }

    fn call(&self, _: &dyn Host, _: &CapabilityName, _: Value) -> Result<Value, PluginError> {
        Ok(Value::Null)
    }

    fn moved(&self) -> &'static [(&'static str, &'static str)] {
        self.0
    }
}

static RENAMED: [&dyn Plugin; 1] = [&Renamed(&[("/controls", "/rules")])];

#[test]
#[should_panic(expected = "a built-in plugin declares a valid moved prefix")]
fn a_built_in_plugin_declaring_the_root_as_a_moved_prefix_fails_as_it_is_built_in() {
    static ROOT: [&dyn Plugin; 1] = [&Renamed(&[("/", "/rules")])];
    open(&board()).build_in(&ROOT);
}

/// A board holding `/rules/p`, and the library it mounts at `/lib`, which
/// holds `/rules/q`.
fn mounting() -> (Scenario, Scenario) {
    let library = Scenario::new();
    let commit = library.commit(&[("rules/q.json", r#"{"$type": "lr/project/v1"}"#)]);
    let scenario = board();
    let mount = format!(
        r#"{{"$type": "board/mount/v1", "repo": "{}", "follow": "main", "commit": "{commit}"}}"#,
        library.path()
    );
    scenario
        .write("lib.json", Item::File(&mount))
        .write("rules/p.json", Item::File(r#"{"$type": "lr/project/v1"}"#));
    (scenario, library)
}

#[test]
fn a_reference_under_a_built_in_plugins_moved_prefix_checks_as_resolved_and_without_it_dangles() {
    let (scenario, _library) = mounting();
    for project in ["/controls/p", "/lib/controls/q"] {
        let task = format!(r#"{{"$type": "lr/task/v1", "title": "t", "project": "{project}"}}"#);
        scenario.write("tasks/t.json", Item::File(&task));
        let status = |board: &Board| {
            let report = board.check(None, None).unwrap();
            let entry = report
                .entries
                .into_iter()
                .find(|entry| entry.path == "tasks/t.json");
            entry.unwrap().status
        };
        let mut moving = open(&scenario);
        moving.build_in(&RENAMED);
        assert_eq!(status(&moving), Status::Ok, "{project}");
        assert_eq!(
            status(&open(&scenario)),
            Status::Degraded(Code::RefDangling),
            "{project}"
        );
    }
}

/// A built-in plugin `rules`, formerly `controls`, shipping
/// `rules/rule/v1`, and `controls/control/v1` which it replaces. Its one
/// capability fails with the code its input names.
struct Retyped;

const OLD: &str = "controls/control/v1";
const NEW: &str = "rules/rule/v1";

impl Plugin for Retyped {
    fn manifest(&self) -> Manifest {
        let mut manifest: Value = serde_json::from_str(MANIFEST).unwrap();
        manifest["name"] = "rules".into();
        manifest["former_names"] = serde_json::json!(["controls"]);
        manifest["types"] = serde_json::json!([NEW, OLD]);
        manifest.as_object_mut().unwrap().remove("run");
        Manifest::built_in(manifest).unwrap()
    }

    fn schemas(&self) -> Vec<(RecordType, Value)> {
        [NEW, OLD]
            .map(|name| (name.parse().unwrap(), serde_json::json!({"type": "object"})))
            .into()
    }

    fn call(&self, _: &dyn Host, _: &CapabilityName, input: Value) -> Result<Value, PluginError> {
        Err(PluginError {
            code: input["code"].as_str().unwrap().to_owned(),
            message: "failed".to_owned(),
            fields: serde_json::Map::new(),
            hint: None,
        })
    }

    fn renamed_types(&self) -> &'static [(&'static str, &'static str)] {
        &[(OLD, NEW)]
    }
}

static RETYPED: [&dyn Plugin; 1] = [&Retyped];

/// The schemas `plugin list` finds missing for the plugin, by type.
fn missing(scenario: &Scenario) -> Vec<String> {
    let mut board = open(scenario);
    board.build_in(&RETYPED);
    let report = board.plugin_list(None).unwrap();
    (report.plugins[0].schemas.iter())
        .map(|problem| format!("{} {}", problem.record_type, problem.code.as_str()))
        .collect()
}

#[test]
fn a_board_holding_either_type_of_a_renamed_pair_lacks_no_schema_and_one_holding_neither_lacks_both()
 {
    let enabling = || {
        let scenario = board();
        let config = r#"{"$type": "board/config/v1", "plugins": ["controls"]}"#;
        scenario.write(".board/board.json", Item::File(config));
        scenario
    };
    let missing_code = Code::PluginSchemaMissing.as_str();
    assert_eq!(
        missing(&enabling()),
        [
            format!("{NEW} {missing_code}"),
            format!("{OLD} {missing_code}")
        ]
    );
    for held in [OLD, NEW] {
        let scenario = enabling();
        let schema = format!(".board/types/{held}.schema.json");
        scenario.write(&schema, Item::File(r#"{"type":"object"}"#));
        assert_eq!(missing(&scenario), Vec::<String>::new(), "{held}");
    }
}

#[test]
fn a_former_name_enables_the_renamed_plugin_under_that_name_with_every_schema_it_ships() {
    let scenario = board();
    scenario.identified();
    let mut board = open(&scenario);
    board.build_in(&RETYPED);
    let former = "controls".parse().unwrap();
    let report = board.plugin_enable(None, None, &former, &[]).unwrap();
    assert_eq!(report.name, former);
    let config = std::fs::read_to_string(scenario.workdir().join(".board/board.json")).unwrap();
    let config: Value = serde_json::from_str(&config).unwrap();
    assert_eq!(config["plugins"], serde_json::json!(["controls"]));
    for name in [NEW, OLD] {
        let schema = scenario
            .workdir()
            .join(format!(".board/types/{name}.schema.json"));
        assert!(schema.exists(), "{name}");
    }
}

#[test]
fn enabling_a_renamed_plugin_by_either_name_keeps_the_one_name_the_board_lists() {
    for (listed, given) in [("controls", "rules"), ("rules", "controls")] {
        let scenario = board();
        scenario.identified();
        let config = format!(r#"{{"$type": "board/config/v1", "plugins": ["{listed}"]}}"#);
        scenario.write(".board/board.json", Item::File(&config));
        let mut board = open(&scenario);
        board.build_in(&RETYPED);
        let given = given.parse().unwrap();
        board.plugin_enable(None, None, &given, &[]).unwrap();
        let config = std::fs::read_to_string(scenario.workdir().join(".board/board.json")).unwrap();
        let config: Value = serde_json::from_str(&config).unwrap();
        assert_eq!(config["plugins"], serde_json::json!([listed]), "{given}");
    }
}

#[test]
fn a_renamed_plugins_code_under_its_former_name_is_bad_output_and_under_its_name_is_its_own() {
    let scenario = board();
    scenario.identified();
    let mut board = open(&scenario);
    board.build_in(&RETYPED);
    board
        .plugin_enable(None, None, &"rules".parse().unwrap(), &[])
        .unwrap();
    let mut board = open(&scenario);
    board.build_in(&RETYPED);
    let code = |given: &str| {
        let input = serde_json::json!({ "code": given }).to_string();
        let called = board.call(
            &"rules.c".parse().unwrap(),
            input.as_bytes(),
            &Plugins::default(),
        );
        called.unwrap_err().refusal().code.as_str().to_owned()
    };
    assert_eq!(code("controls::c::old"), Code::PluginBadOutput.as_str());
    assert_eq!(code("rules::c::new"), "rules::c::new");
}
