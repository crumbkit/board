//! The manifest parses as Board reads it, the schema file is what its
//! struct generates and admits a map and refuses its malformed twins, and
//! `map.check` reads a map from a real board and refuses what is not one.

use std::path::Path;

use board_model::canonical_json;
use board_ops::{Board, Plugins};
use board_plugin::Target;
use board_scenario::{Item, Scenario};
use serde_json::{Value, json};

use super::*;

/// A fixture map, with the `$type` the files leave out so that `board
/// check` in Board's own repository does not read them as records.
pub(crate) fn fixture(name: &str) -> Value {
    let file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(format!("{name}.json"));
    let mut map: Value = serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
    map["$type"] = json!(LayoutRecord::TYPE);
    map
}

/// The `description` of SKILL.md's frontmatter, a single-quoted YAML
/// scalar on one line.
fn skill_description() -> String {
    let skill = include_str!("../../../plugins/map/SKILL.md");
    let line = (skill.lines())
        .find_map(|line| line.strip_prefix("description: "))
        .unwrap();
    let quoted = line.strip_prefix('\'').unwrap().strip_suffix('\'').unwrap();
    quoted.replace("''", "'")
}

#[test]
fn the_manifest_parses_and_names_its_type_and_capability() {
    let manifest = MapPlugin.manifest();
    assert_eq!(manifest.description, skill_description());
    assert_eq!(manifest.name.as_str(), "map");
    let capabilities: Vec<&str> = (manifest.capabilities.iter())
        .map(|capability| capability.name.as_str())
        .collect();
    assert_eq!(capabilities, ["check", "place"]);
    assert_eq!(
        (MapPlugin.schemas().into_iter())
            .map(|(name, _)| name.as_str().to_owned())
            .collect::<Vec<_>>(),
        ["map/layout/v1"]
    );
}

/// The committed schema file is the schema the struct generates.
/// `SNAPSHOTS=overwrite` rewrites it.
#[expect(
    clippy::disallowed_methods,
    reason = "the schema files are goldens, selected and written as crumbkit-test's are"
)]
#[test]
fn the_committed_layout_schema_is_what_its_struct_generates() {
    let types = Path::new(env!("CARGO_MANIFEST_DIR")).join("types");
    let overwrite = std::env::var("SNAPSHOTS").as_deref() == Ok("overwrite");
    for (record_type, schema) in MapPlugin.schemas() {
        let file = types.join(format!("{}.schema.json", record_type.as_str()));
        let generated = canonical_json(&schema);
        if overwrite {
            std::fs::write(&file, generated.as_bytes()).unwrap();
        }
        let committed = std::fs::read(&file).unwrap();
        assert!(
            committed == generated.as_bytes(),
            "{} differs from what its struct generates; run this test with SNAPSHOTS=overwrite:\n{}",
            file.display(),
            String::from_utf8_lossy(generated.as_bytes())
        );
    }
}

/// Every fixture is a map the schema admits, and each malformed field is
/// refused in an otherwise identical twin.
#[test]
fn a_map_with_a_malformed_field_is_refused_and_its_twin_is_admitted() {
    let schema = board_validate::Schema::compile(layout_schema()).unwrap();
    for name in ["services", "work", "queue"] {
        for variant in ["placed", "final"] {
            schema
                .validate(&fixture(&format!("{name}-{variant}")))
                .unwrap();
        }
    }
    let map = fixture("services-final");
    let mut map = map;
    map["accepts"] = json!([{"kind": "label-overlap", "message": "m", "reason": "r"}]);
    schema.validate(&map).unwrap();
    let refused: [(&str, Value); 15] = [
        ("/title", json!("")),
        ("/sheet/width", json!(0)),
        ("/regions", json!([])),
        ("/regions/0/id", json!("Not A Slug")),
        ("/items", json!([])),
        ("/items/0/label", json!([])),
        ("/items/0/radius", json!(0)),
        ("/items/0/at/set", json!("moved")),
        ("/items/0/side", json!("top")),
        ("/lines/0/kind", json!("")),
        ("/notes/0/anchor/line/along", json!(1.5)),
        ("/notes/0/anchor", json!({"point": {"x": 1, "y": 2}})),
        ("/accepts", json!([])),
        ("/accepts/0/kind", json!("label-clash")),
        ("/accepts/0/reason", json!("")),
    ];
    for (pointer, value) in refused {
        let mut twin = map.clone();
        *twin.pointer_mut(pointer).unwrap() = value;
        assert!(schema.validate(&twin).is_err(), "{pointer}");
    }
    let mut extra = map.clone();
    extra["items"][0]["colour"] = json!("blue");
    assert!(schema.validate(&extra).is_err());
    let mut unplaced = map;
    let item = unplaced["items"][0].as_object_mut().unwrap();
    item.remove("at");
    item.remove("side");
    schema.validate(&unplaced).unwrap();
}

/// A board that enables map and holds `map` at /maps/m.json, beside its
/// config at /.board/board.json, a record that is not a map.
pub(crate) fn board_with(map: &Value) -> (Scenario, Board) {
    let scenario = Scenario::new();
    scenario.write(
        ".board/board.json",
        Item::File(r#"{"$type": "board/config/v1", "commit_on_put": false}"#),
    );
    scenario.write("maps/m.json", Item::File(&map.to_string()));
    let mut board = Board::open(scenario.workdir(), None).unwrap();
    board.build_in(&[&MapPlugin]);
    board
        .plugin_enable(None, None, &"map".parse().unwrap(), &[])
        .unwrap();
    let mut board = Board::open(scenario.workdir(), None).unwrap();
    board.build_in(&[&MapPlugin]);
    (scenario, board)
}

/// The output of `map.check` on `input`, or the code it was refused with.
fn called(board: &mut Board, input: &Value) -> Result<Value, String> {
    called_as(board, "map.check", input)
}

/// The output of `target` on `input`, or the code it was refused with.
pub(crate) fn called_as(board: &mut Board, target: &str, input: &Value) -> Result<Value, String> {
    let target: Target = target.parse().unwrap();
    let answer = board.call(&target, input.to_string().as_bytes(), &Plugins::default());
    let output = answer
        .map(|report| report.output)
        .map_err(|failed| failed.refusal().code.as_str().to_owned());
    let started = board.take_trace().work.get("started").copied();
    assert_eq!(started, Some(0), "{output:?}");
    output
}

#[test]
fn check_reads_the_map_from_the_board_and_bounds_its_findings() {
    let (_scenario, mut board) = board_with(&fixture("work-placed"));
    let all = called(&mut board, &json!({"path": "/maps/m"})).unwrap();
    assert_eq!(all["problems"], 12);
    assert_eq!(all["findings"].as_array().unwrap().len(), 12);
    assert_eq!(all["left_out"], 0);
    let some = called(&mut board, &json!({"path": "/maps/m", "limit": 5})).unwrap();
    assert_eq!(some["problems"], 12);
    assert_eq!(
        some["findings"],
        json!(all["findings"].as_array().unwrap()[..5])
    );
    assert_eq!(some["left_out"], 7);
}

#[test]
fn check_refuses_a_path_with_no_map_and_admits_one_with_a_map() {
    let (_scenario, mut board) = board_with(&fixture("work-final"));
    let admitted = called(&mut board, &json!({"path": "/maps/m"})).unwrap();
    assert_eq!(admitted["problems"], 0);
    for (path, code) in [
        ("/maps/none", "map::layout::not_found"),
        ("/.board/board", "map::layout::unreadable"),
    ] {
        assert_eq!(
            called(&mut board, &json!({"path": path})),
            Err(code.to_owned()),
            "{path}"
        );
    }
}

/// A finding the map accepts is counted apart and not listed; an accept
/// whose message no longer matches leaves the finding listed and is itself
/// listed as unused.
#[test]
fn check_counts_an_accepted_finding_apart_and_lists_an_unused_accept() {
    let message = "the label of alder overlaps the label of cedar";
    let mut accepting = fixture("services-placed");
    accepting["accepts"] = json!([
        {"kind": "label-overlap", "message": message, "reason": "the two read as one pair"}
    ]);
    let mut stale = accepting.clone();
    stale["accepts"][0]["message"] = json!("the label of alder overlaps the label of elm");
    let (_scenario, mut board) = board_with(&accepting);
    let accepted = called(&mut board, &json!({"path": "/maps/m"})).unwrap();
    assert_eq!(
        (accepted["problems"].clone(), accepted["accepted"].clone()),
        (json!(54), json!(1))
    );
    let listed = |output: &Value, text: &str| {
        (output["findings"].as_array().unwrap().iter())
            .any(|finding| finding["message"].as_str().unwrap().contains(text))
    };
    assert!(!listed(&accepted, message));
    let (_scenario, mut board) = board_with(&stale);
    let unused = called(&mut board, &json!({"path": "/maps/m"})).unwrap();
    assert_eq!(
        (unused["problems"].clone(), unused["accepted"].clone()),
        (json!(56), json!(0))
    );
    assert!(listed(&unused, message));
    let accept = &unused["findings"][0];
    assert_eq!(accept["kind"], "accept-unused");
    assert!(accept["message"].as_str().unwrap().contains("label of elm"));
}

#[test]
fn an_unknown_capability_is_refused_before_the_plugin_runs() {
    let (_scenario, board) = board_with(&fixture("work-final"));
    let target: Target = "map.nothing".parse().unwrap();
    let code = (board.call(&target, b"{}", &Plugins::default()))
        .map(|_| ())
        .map_err(|failed| failed.refusal().code.as_str().to_owned());
    assert_eq!(code, Err("board::plugin::no_capability".to_owned()));
}
