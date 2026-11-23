//! Placement from no stored positions clears the checker on the three
//! fixture maps, gives one answer whatever the order of the map's parts,
//! never moves a pinned item, and keeps notes with their anchors.

use serde_json::{Value, json};

use crate::geom::Box2;

use super::*;
use crate::tests::{board_with, called_as, fixture};

/// A fixture map with every stored position, side and note offset removed.
fn bare(name: &str) -> Value {
    let mut map = fixture(&format!("{name}-final"));
    for item in map["items"].as_array_mut().unwrap() {
        let item = item.as_object_mut().unwrap();
        item.remove("at");
        item.remove("side");
    }
    for note in map["notes"].as_array_mut().unwrap() {
        if let Some(line) = note["anchor"].get_mut("line") {
            line.as_object_mut().unwrap().remove("offset");
        }
    }
    map
}

fn read(value: &Value) -> LayoutRecord {
    serde_json::from_value(value.clone()).unwrap()
}

fn placed(map: &mut LayoutRecord, relayout: bool) -> Run {
    let mut run = Run::new(map, relayout);
    run.place(map);
    run
}

fn state(map: &LayoutRecord) -> Value {
    serde_json::to_value(map).unwrap()
}

/// Each item's id with its stored position and side, sorted by id.
type Spot = (String, Option<(f64, f64)>, Option<Side>);

fn spots_of(map: &LayoutRecord) -> Vec<Spot> {
    let mut spots: Vec<_> = (map.items.iter())
        .map(|item| (item.id.clone(), item.at.map(|at| (at.x, at.y)), item.side))
        .collect();
    spots.sort_by(|a, b| a.0.cmp(&b.0));
    spots
}

#[test]
fn placing_a_bare_map_leaves_no_findings() {
    for name in ["services", "work", "queue"] {
        let mut map = read(&bare(name));
        assert!(!check(&map).unwrap().is_empty(), "{name} starts unplaced");
        placed(&mut map, false);
        let left: Vec<String> = (check(&map).unwrap().into_iter())
            .map(|finding| finding.message)
            .collect();
        assert_eq!(left, Vec::<String>::new(), "{name}");
    }
}

/// Reversing the order of every list in the map gives the same positions
/// and sides.
#[test]
fn the_order_of_a_maps_parts_changes_no_position_or_side() {
    for name in ["services", "work", "queue"] {
        let forward = bare(name);
        let mut backward = forward.clone();
        for list in ["regions", "items", "lines", "notes"] {
            backward[list].as_array_mut().unwrap().reverse();
        }
        let (mut one, mut two) = (read(&forward), read(&backward));
        placed(&mut one, false);
        placed(&mut two, false);
        assert_eq!(spots_of(&one), spots_of(&two), "{name}");
    }
}

/// A run moves no pinned item and no engine item, places only the item
/// with no position, and a second run changes nothing.
#[test]
fn a_run_keeps_pinned_and_engine_items_and_places_only_what_has_no_position() {
    let mut value = fixture("services-final");
    let held = spots_of(&read(&value));
    value["items"][0].as_object_mut().unwrap().remove("at");
    value["items"][0].as_object_mut().unwrap().remove("side");
    let id = value["items"][0]["id"].as_str().unwrap().to_owned();
    value["items"][1]["at"]["set"] = json!("engine");
    let mut map = read(&value);
    let run = placed(&mut map, false);
    assert_eq!(run.placed(), std::slice::from_ref(&id));
    let now = spots_of(&map);
    for (one, two) in held.iter().zip(&now).filter(|(one, _)| one.0 != id) {
        assert_eq!((&one.1, one.2), (&two.1, two.2), "{}", one.0);
    }
    assert!(map.items[0].at.is_some_and(|at| at.set == Set::Engine));
    let after = state(&map);
    placed(&mut map, false);
    assert_eq!(state(&map), after);
}

/// With relayout an engine item may move and a pinned one never does.
#[test]
fn relayout_moves_engine_items_and_never_pinned_ones() {
    let mut value = bare("work");
    let mut map = read(&value);
    placed(&mut map, false);
    value = state(&map);
    let pinned = value["items"][0]["id"].as_str().unwrap().to_owned();
    value["items"][0]["at"]["set"] = json!("pinned");
    // Shift every engine item, so a relayout has something to undo.
    for item in value["items"].as_array_mut().unwrap().iter_mut().skip(1) {
        item["at"]["x"] = json!(item["at"]["x"].as_f64().unwrap() + 3.0);
    }
    let before = read(&value);
    let mut map = read(&value);
    let run = placed(&mut map, true);
    let (was, now) = (spots_of(&before), spots_of(&map));
    let held = |spots: &[Spot]| spots.iter().find(|spot| spot.0 == pinned).cloned();
    assert_eq!(held(&was), held(&now));
    assert!(!run.moved(&map).contains(&pinned));
    assert_ne!(run.moved(&map).len(), 0);
}

/// A note anchored to an item moves with it: the same anchor puts the
/// note's box the same distance from the item's mark wherever the item is.
#[test]
fn a_note_stays_with_its_item_after_the_item_moves() {
    let value = fixture("work-final");
    let mut moved = value.clone();
    let anchored = (value["notes"].as_array().unwrap().iter())
        .find_map(|note| note["anchor"]["item"]["id"].as_str().map(str::to_owned))
        .unwrap();
    let at = moved["items"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|item| item["id"] == json!(anchored))
        .unwrap();
    at["at"]["x"] = json!(at["at"]["x"].as_f64().unwrap() + 70.0);
    let notes = |value: &Value| {
        let map = read(value);
        crate::check::notes_of(&map)
    };
    let (one, two) = (notes(&value), notes(&moved));
    let delta = |a: &Box2, b: &Box2| (b.x0 - a.x0, b.y0 - a.y0);
    let first = (value["notes"].as_array().unwrap().iter())
        .position(|note| note["anchor"]["item"]["id"] == json!(anchored))
        .unwrap();
    assert_eq!(delta(&one[first], &two[first]), (70.0, 0.0));
}

/// `map.place` on a board writes the placed map in one put, so `map.check`
/// then finds nothing, and a second run writes nothing and reports nothing
/// placed.
#[test]
fn place_writes_the_map_and_a_second_run_changes_nothing() {
    let (scenario, mut board) = board_with(&bare("work"));
    let input = json!({"path": "/maps/m"});
    let first = called_as(&mut board, "map.place", &input).unwrap();
    assert_eq!(first["problems"], 0);
    assert_eq!(first["placed"].as_array().unwrap().len(), 13);
    let checked = called_as(&mut board, "map.check", &input).unwrap();
    assert_eq!(checked["problems"], 0);
    let stored = std::fs::read(scenario.workdir().join("maps/m.json")).unwrap();
    let second = called_as(&mut board, "map.place", &input).unwrap();
    assert_eq!(second["placed"], json!([]));
    assert_eq!(second["moved"], json!([]));
    assert_eq!(second["notes_placed"], first["notes_placed"]);
    assert_eq!(
        std::fs::read(scenario.workdir().join("maps/m.json")).unwrap(),
        stored
    );
}

/// Two pinned items on one spot cannot be cleared: the run stores the best
/// spots for the rest and refuses with the findings left, and its twin
/// with the items apart is admitted.
#[test]
fn place_stores_its_best_spots_and_refuses_when_findings_are_left() {
    let mut stuck = bare("work");
    for item in stuck["items"].as_array_mut().unwrap().iter_mut().take(2) {
        item["at"] = json!({"x": 100, "y": 100, "set": "pinned"});
    }
    let (scenario, mut board) = board_with(&stuck);
    let input = json!({"path": "/maps/m"});
    assert_eq!(
        called_as(&mut board, "map.place", &input),
        Err("map::place::findings_left".to_owned())
    );
    let stored: Value =
        serde_json::from_slice(&std::fs::read(scenario.workdir().join("maps/m.json")).unwrap())
            .unwrap();
    assert!(stored["items"][2]["at"].is_object());
    let mut apart = bare("work");
    let done = fixture("work-final");
    for at in 0..2 {
        apart["items"][at]["at"] = done["items"][at]["at"].clone();
    }
    let (_scenario, mut board) = board_with(&apart);
    let output = called_as(&mut board, "map.place", &input).unwrap();
    assert_eq!(output["problems"], 0);
}

/// A path with no map and a record that is not a map are refused with the
/// codes `map.check` gives.
#[test]
fn place_refuses_what_is_not_a_map() {
    let (_scenario, mut board) = board_with(&bare("work"));
    for (path, code) in [
        ("/maps/none", "map::layout::not_found"),
        ("/.board/board", "map::layout::unreadable"),
    ] {
        assert_eq!(
            called_as(&mut board, "map.place", &json!({"path": path})),
            Err(code.to_owned()),
            "{path}"
        );
    }
}
