//! A built-in plugin's moved prefixes: a reference under either prefix
//! resolves while the board holds its target under the other
//! (docs/plugins/calling/commands.md, Renamed plugins).

use board_model::{CapabilityName, Code, RecordType};
use board_plugin::Manifest;
use board_scenario::{Item, Scenario};
use board_tree::Status;
use serde_json::Value;

use super::{MANIFEST, board, open};
use crate::Board;
use crate::host::{Host, Plugin, PluginError};

/// A built-in plugin whose records moved from `/controls` to `/rules`. Its
/// pair naming the root is one `Moved::new` refuses, which is skipped.
struct Renamed;

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
        &[("/", "/rules"), ("/controls", "/rules")]
    }
}

static RENAMED: [&dyn Plugin; 1] = [&Renamed];

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
