//! The checker finds exactly what the prototype's checker found on the
//! three fixture maps, refuses a map that names what it does not hold, and
//! reads notes from their anchors.

use std::path::Path;

use serde_json::{Value, json};

use super::*;
use crate::tests::fixture;

/// The map `value` holds.
fn map(value: &Value) -> LayoutRecord {
    serde_json::from_value(value.clone()).unwrap()
}

/// What `check` finds in `value`, each as its kind and message, sorted.
fn found(value: &Value) -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = (check(&map(value)).unwrap().into_iter())
        .map(|finding| {
            let kind = serde_json::to_value(finding.kind).unwrap();
            (kind.as_str().unwrap().to_owned(), finding.message)
        })
        .collect();
    found.sort();
    found
}

/// `fixtures/findings.json` holds, for each fixture, what the prototype's
/// checker (`geom.js` in /projects/board/prototypes/map-overlays/real-maps/)
/// found in it, worded as `check` words it. With the prototype's placement
/// the three maps have 55, 12 and 35 problems; their final layouts have
/// none.
#[test]
fn check_finds_what_the_prototype_checker_found_on_each_fixture() {
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/findings.json");
    let expected: Value = serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
    let mut counts = Vec::new();
    for (name, rows) in expected.as_object().unwrap() {
        let mut rows: Vec<(String, String)> = serde_json::from_value(rows.clone()).unwrap();
        rows.sort();
        assert_eq!(found(&fixture(name)), rows, "{name}");
        counts.push((name.as_str(), rows.len()));
    }
    counts.sort_unstable();
    assert_eq!(
        counts,
        [
            ("queue-final", 0),
            ("queue-placed", 35),
            ("services-final", 0),
            ("services-placed", 55),
            ("work-final", 0),
            ("work-placed", 12),
        ]
    );
}

/// Reversing the order of the items, lines and notes changes no finding.
#[test]
fn the_order_of_a_maps_parts_changes_no_finding() {
    for name in ["services-placed", "work-placed", "queue-placed"] {
        let forward = fixture(name);
        let mut reversed = forward.clone();
        for list in ["regions", "items", "lines", "notes"] {
            reversed[list].as_array_mut().unwrap().reverse();
        }
        let message = |rows: Vec<(String, String)>| {
            // A pair is named in the order the map lists it, so compare
            // each message's names as a set.
            let mut rows: Vec<(String, Vec<String>)> = (rows.into_iter())
                .map(|(kind, message)| {
                    let mut words: Vec<String> =
                        message.split([' ', '→']).map(str::to_owned).collect();
                    words.sort();
                    (kind, words)
                })
                .collect();
            rows.sort();
            rows
        };
        assert_eq!(
            message(found(&forward)),
            message(found(&reversed)),
            "{name}"
        );
    }
}

/// A map whose parts name what it does not hold, or repeat a name, is
/// refused; its twin that holds what it names is admitted.
#[test]
fn a_map_naming_what_it_does_not_hold_is_refused_and_its_twin_is_admitted() {
    let good = fixture("services-final");
    check(&map(&good)).unwrap();
    let first = good["items"][0]["id"].as_str().unwrap().to_owned();
    let cases: [(&str, Value, &str); 5] = [
        (
            "/items/0/region",
            json!("nowhere"),
            "map::layout::unknown_id",
        ),
        ("/lines/0/to", json!("nothing"), "map::layout::unknown_id"),
        (
            "/notes/0/anchor",
            json!({"line": {"from": first, "to": first, "along": 0.5}}),
            "map::layout::unknown_id",
        ),
        ("/items/1/id", json!(first), "map::layout::duplicate_id"),
        (
            "/regions/1/id",
            good["regions"][0]["id"].clone(),
            "map::layout::duplicate_id",
        ),
    ];
    for (pointer, value, code) in cases {
        let mut twin = good.clone();
        *twin.pointer_mut(pointer).unwrap() = value;
        let refused = check(&map(&twin)).map(|_| ()).map_err(|error| error.code);
        assert_eq!(refused, Err(code.to_owned()), "{pointer}");
    }
}

/// A lane names a region and, with a region, must name the same one; each
/// refusal has an admitted twin.
#[test]
fn a_lane_must_name_a_region_and_agree_with_the_items_region() {
    let mut good = fixture("queue-final");
    let region = good["items"][0]["region"].as_str().unwrap().to_owned();
    good["items"][0]["lane"] = json!(region);
    good["items"][0]["column"] = json!(2);
    check(&map(&good)).unwrap();
    let other = good["regions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|region| region["id"].as_str().unwrap().to_owned())
        .find(|id| *id != region)
        .unwrap();
    for (lane, code) in [
        (json!("nowhere"), "map::layout::unknown_id"),
        (json!(other), "map::layout::lane_region_mismatch"),
    ] {
        let mut twin = good.clone();
        twin["items"][0]["lane"] = lane;
        let refused = check(&map(&twin)).map(|_| ()).map_err(|error| error.code);
        assert_eq!(refused, Err(code.to_owned()));
    }
    // With no region of its own, an item's lane is its region.
    let mut laned = good.clone();
    laned["items"][0].as_object_mut().unwrap().remove("region");
    check(&map(&laned)).unwrap();
}

/// A negative column is refused by the schema; the twin with column 0 is
/// admitted.
#[test]
fn the_schema_refuses_a_negative_column() {
    let mut good = fixture("queue-final");
    good["items"][0]["lane"] = good["items"][0]["region"].clone();
    good["items"][0]["column"] = json!(0);
    let schema = board_validate::Schema::compile(crate::layout_schema()).unwrap();
    schema.validate(&good).unwrap();
    good["items"][0]["column"] = json!(-1);
    let refused = schema.validate(&good).unwrap_err();
    assert_eq!(refused.code(), board_model::Code::RecordInvalid);
    assert_eq!(
        refused.reason(),
        "fails its schema at \"/items/0/column\": -1 is less than the minimum of 0"
    );
}

/// A `line_kinds` entry names a kind some line has; an entry for a kind no
/// line has is refused, and its twin naming a kind a line has is admitted.
#[test]
fn a_line_kind_no_line_uses_is_refused_and_its_twin_is_admitted() {
    let mut good = fixture("services-final");
    good["line_kinds"] = json!([{"kind": "lib", "label": "builds on"}]);
    check(&map(&good)).unwrap();
    let mut twin = good.clone();
    twin["line_kinds"][0]["kind"] = json!("ferry");
    let refused = check(&map(&twin)).map(|_| ()).unwrap_err();
    assert_eq!(refused.code, "map::layout::unknown_id");
    assert_eq!(
        refused.message,
        "the map styles the line kind ferry, which no line of the map has"
    );
}

/// An item with no stored position is reported and left out of the
/// drawing, with its lines and notes.
#[test]
fn an_item_with_no_position_is_reported_and_its_lines_are_not_drawn() {
    let mut value = fixture("services-final");
    let id = value["items"][0]["id"].as_str().unwrap().to_owned();
    value["items"][0].as_object_mut().unwrap().remove("at");
    assert_eq!(
        found(&value),
        [(
            "item-unplaced".to_owned(),
            format!("{id} has no stored position, so the map does not draw it")
        )]
    );
}

/// A note anchored to an item moves with it. Moving every item 70 units
/// right and 70 down gives no note finding, where a note stored at fixed
/// coordinates would be 99 units from its item, past the 70 a note may be.
#[test]
fn a_note_stays_with_its_item_when_the_item_moves() {
    let mut value = fixture("work-final");
    let notes = value["notes"].as_array().unwrap().clone();
    assert!(
        notes
            .iter()
            .any(|note| note["anchor"].get("item").is_some())
    );
    for item in value["items"].as_array_mut().unwrap() {
        for axis in ["x", "y"] {
            let at = item["at"][axis].as_f64().unwrap();
            item["at"][axis] = json!(at + 70.0);
        }
    }
    for side in ["width", "height"] {
        value["sheet"][side] = json!(value["sheet"][side].as_f64().unwrap() + 70.0);
    }
    value.as_object_mut().unwrap().remove("regions");
    for item in value["items"].as_array_mut().unwrap() {
        item.as_object_mut().unwrap().remove("region");
    }
    let notes: Vec<(String, String)> = (found(&value).into_iter())
        .filter(|(kind, _)| kind.starts_with("note-"))
        .collect();
    assert_eq!(notes, Vec::<(String, String)>::new());
}

/// A finding names the box the checker estimated for each label it names,
/// so a reader can compare the estimate with the drawn text.
#[test]
fn a_finding_gives_the_estimated_box_of_each_label_it_names() {
    let findings = check(&map(&fixture("services-placed"))).unwrap();
    let overlap = (findings.iter())
        .find(|finding| finding.message == "the label of alder overlaps the label of cedar")
        .unwrap();
    let named: Vec<&str> = (overlap.estimated.iter())
        .map(|area| area.of.as_str())
        .collect();
    assert_eq!(named, ["the label of alder", "the label of cedar"]);
    let [a, b] = [&overlap.estimated[0], &overlap.estimated[1]];
    assert!(a.x0 < b.x1 && b.x0 < a.x1 && a.y0 < b.y1 && b.y0 < a.y1);
    let crossing = (findings.iter())
        .find(|finding| finding.kind == Kind::LinesShareAngle)
        .unwrap();
    assert!(crossing.estimated.is_empty());
}
