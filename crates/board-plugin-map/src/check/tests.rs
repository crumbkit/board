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
        assert_eq!(message(found(&forward)), message(found(&reversed)), "{name}");
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
        ("/items/0/region", json!("nowhere"), "map::layout::unknown_id"),
        ("/lines/0/to", json!("nothing"), "map::layout::unknown_id"),
        (
            "/notes/0/anchor",
            json!({"line": {"from": first, "to": first, "along": 0.5}}),
            "map::layout::unknown_id",
        ),
        ("/items/1/id", json!(first), "map::layout::duplicate_id"),
        ("/regions/1/id", good["regions"][0]["id"].clone(), "map::layout::duplicate_id"),
    ];
    for (pointer, value, code) in cases {
        let mut twin = good.clone();
        *twin.pointer_mut(pointer).unwrap() = value;
        let refused = check(&map(&twin)).map(|_| ()).map_err(|error| error.code);
        assert_eq!(refused, Err(code.to_owned()), "{pointer}");
    }
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
    assert!(notes.iter().any(|note| note["anchor"].get("item").is_some()));
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
