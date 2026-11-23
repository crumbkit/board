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
    assert_eq!(run.placed(&map), std::slice::from_ref(&id));
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

/// The fixture's items given a lane (their region) and a column (the rank
/// of their x among the lane's distinct x values), with no stored
/// positions.
fn laned(name: &str) -> Value {
    let done = fixture(&format!("{name}-final"));
    let mut map = bare(name);
    for at in 0..map["items"].as_array().unwrap().len() {
        let region = done["items"][at]["region"].clone();
        let mut xs: Vec<f64> = (done["items"].as_array().unwrap().iter())
            .filter(|item| item["region"] == region)
            .map(|item| item["at"]["x"].as_f64().unwrap())
            .collect();
        xs.sort_by(f64::total_cmp);
        xs.dedup();
        let x = done["items"][at]["at"]["x"].as_f64().unwrap();
        map["items"][at]["lane"] = region;
        map["items"][at]["column"] =
            json!(xs.iter().position(|v| v.total_cmp(&x).is_eq()).unwrap());
    }
    map
}

/// Each lane's items, as (column, x) sorted by column then x.
fn lanes(map: &LayoutRecord) -> std::collections::BTreeMap<String, Vec<(u32, f64)>> {
    let mut lanes = std::collections::BTreeMap::<String, Vec<(u32, f64)>>::new();
    for item in &map.items {
        let (Some(lane), Some(column), Some(at)) = (&item.lane, item.column, item.at) else {
            continue;
        };
        lanes.entry(lane.clone()).or_default().push((column, at.x));
    }
    for items in lanes.values_mut() {
        items.sort_by(|a, b| a.partial_cmp(b).unwrap());
    }
    lanes
}

/// Placing a laned map clears the checker, keeps each item inside its
/// lane's region, and puts a lane's columns left to right.
#[test]
fn placing_a_laned_map_keeps_each_lane_in_column_order() {
    for name in ["services", "work", "queue"] {
        let mut map = read(&laned(name));
        placed(&mut map, false);
        let left: Vec<String> = (check(&map).unwrap().into_iter())
            .map(|finding| finding.message)
            .collect();
        assert_eq!(left, Vec::<String>::new(), "{name}");
        for (lane, items) in lanes(&map) {
            for pair in items.windows(2) {
                assert!(
                    pair[0].0 == pair[1].0 || pair[0].1 < pair[1].1,
                    "{name} {lane}: {pair:?}"
                );
            }
        }
        for item in &map.items {
            let region = (map.regions.iter())
                .find(|region| Some(&region.id) == item.lane.as_ref())
                .unwrap();
            let at = item.at.unwrap();
            assert!(
                (region.x..=region.x + region.width).contains(&at.x)
                    && (region.y..=region.y + region.height).contains(&at.y),
                "{name} {}",
                item.id
            );
        }
    }
}

/// An item whose only region is its lane is placed inside that lane.
#[test]
fn an_item_with_a_lane_and_no_region_is_placed_in_the_lane() {
    let mut value = laned("queue");
    for item in value["items"].as_array_mut().unwrap() {
        item.as_object_mut().unwrap().remove("region");
    }
    let mut map = read(&value);
    placed(&mut map, false);
    for item in &map.items {
        let region = (map.regions.iter())
            .find(|region| Some(&region.id) == item.lane.as_ref())
            .unwrap();
        let at = item.at.unwrap();
        assert!(
            (region.y..=region.y + region.height).contains(&at.y),
            "{}",
            item.id
        );
    }
}

/// Columns do not change the order-independence of placement.
#[test]
fn the_order_of_a_laned_maps_parts_changes_no_position() {
    let forward = laned("queue");
    let mut backward = forward.clone();
    for list in ["regions", "items", "lines", "notes"] {
        backward[list].as_array_mut().unwrap().reverse();
    }
    let (mut one, mut two) = (read(&forward), read(&backward));
    placed(&mut one, false);
    placed(&mut two, false);
    assert_eq!(spots_of(&one), spots_of(&two));
}

/// A pinned item in a lane limits where its neighbours may stand: an item
/// of a lower column stays left of it and one of a higher column right.
#[test]
fn a_pinned_item_bounds_the_columns_beside_it() {
    let mut value = laned("queue");
    let done = fixture("queue-final");
    // Pin the second column of the first lane with a stored position.
    let (target, _) = value["items"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .find(|(_, item)| item["column"] == 1)
        .map(|(at, item)| (at, item.clone()))
        .unwrap();
    value["items"][target]["at"] = done["items"][target]["at"].clone();
    let mut map = read(&value);
    placed(&mut map, false);
    let pinned = map.items[target].clone();
    let (lane, x) = (pinned.lane.clone().unwrap(), pinned.at.unwrap().x);
    for item in &map.items {
        if item.lane.as_ref() != Some(&lane) || item.column == pinned.column {
            continue;
        }
        let at = item.at.unwrap();
        if item.column < pinned.column {
            assert!(at.x < x, "{} stays left", item.id);
        } else {
            assert!(at.x > x, "{} stays right", item.id);
        }
    }
}

/// A lane with pinned items in columns 0 and 3 ten units apart leaves the
/// items of columns 1 and 2 no grid position between them. The run lists
/// neither as placed, and its refusal says they could not be placed at all.
#[test]
fn an_item_with_no_room_is_not_listed_as_placed() {
    let item = |id: &str, column: u32, at: Option<f64>| {
        let mut item = json!({"id": id, "label": [id], "lane": "row", "column": column});
        if let Some(x) = at {
            item["at"] = json!({"x": x, "y": 60, "set": "pinned"});
        }
        item
    };
    let value = json!({
        "$type": "map/layout/v1",
        "title": "No room",
        "sheet": {"width": 400, "height": 200},
        "regions": [{"id": "row", "title": "Row", "x": 0, "y": 0, "width": 400, "height": 200}],
        "items": [
            item("a", 0, Some(40.0)),
            item("b", 1, None),
            item("c", 2, None),
            item("d", 3, Some(50.0)),
        ],
    });
    let mut map = read(&value);
    let run = placed(&mut map, false);
    assert!(map.items[1].at.is_none() && map.items[2].at.is_none());
    assert_eq!(run.placed(&map), Vec::<String>::new());
    let (_scenario, board) = board_with(&value);
    let target: board_plugin::Target = "map.place".parse().unwrap();
    let refused = board
        .call(
            &target,
            json!({"path": "/maps/m"}).to_string().as_bytes(),
            &board_ops::Plugins::default(),
        )
        .unwrap_err()
        .refusal();
    assert_eq!(refused.code.as_str(), "map::place::findings_left");
    assert_eq!(refused.fields["result"]["placed"], json!([]));
    assert_eq!(
        refused.hint.as_deref(),
        Some(
            "b and c could not be placed at all: give them room in their region or lane, or \
             pin them with `at.set` pinned, then run map.place again"
        )
    );
}

/// A map whose items are all pinned: one at each of `spots`, named by
/// it, and `far` alone in the sheet's far corner with its label on the
/// right.
fn pinned_map(spots: &[(&str, f64, f64)]) -> Value {
    let item = |id: &str, x: f64, y: f64| {
        json!({"id": id, "label": [format!("item {id}")], "side": "right",
               "at": {"x": x, "y": y, "set": "pinned"}})
    };
    let mut items: Vec<Value> = (spots.iter())
        .map(|&(id, x, y)| item(id, x, y))
        .collect();
    items.push(item("far", 600.0, 400.0));
    json!({
        "$type": "map/layout/v1",
        "title": "Pinned",
        "sheet": {"width": 800, "height": 500},
        "items": items,
    })
}

/// The side of the item `id`.
fn side_of(map: &LayoutRecord, id: &str) -> Option<Side> {
    map.items.iter().find(|item| item.id == id).and_then(|item| item.side)
}

/// Two pinned items on one spot leave a finding no side clears, so the run
/// anneals, and `c`'s label over `d`'s mark is cleared on the way. The run
/// changes no side of an item more than a label width from those.
#[test]
fn annealing_leaves_the_side_of_a_far_item_alone() {
    let mut map = read(&pinned_map(&[
        ("a", 100.0, 100.0),
        ("b", 100.0, 100.0),
        ("c", 300.0, 200.0),
        ("d", 340.0, 200.0),
    ]));
    let run = placed(&mut map, false);
    let left: Vec<Kind> = (check(&map).unwrap().into_iter())
        .map(|finding| finding.kind)
        .collect();
    assert_eq!(left, [Kind::ItemsOverlap]);
    assert!(!run.sides_changed(&map).contains(&"far".to_owned()));
    assert_eq!(side_of(&map, "far"), Some(Side::Right));
}

/// A person moves `a` and pins it just left of `b`, so its right-hand label
/// runs over `b`'s mark. The next run moves nothing and clears the finding
/// by choosing a side again near `a`, and leaves the far item's side alone.
#[test]
fn a_run_after_a_person_moves_a_pinned_item_chooses_sides_near_it_again() {
    let mut map = read(&pinned_map(&[("a", 200.0, 200.0), ("b", 240.0, 200.0)]));
    assert!(problems(&map) > 0, "the moved label overlaps");
    let before = spots_of(&map);
    let run = placed(&mut map, false);
    assert_eq!(problems(&map), 0);
    let after = spots_of(&map);
    for (was, now) in before.iter().zip(&after) {
        assert_eq!(was.1, now.1, "{} keeps its position", was.0);
    }
    let changed = run.sides_changed(&map);
    assert!(!changed.is_empty(), "a side changed");
    assert!(changed.iter().all(|id| id == "a" || id == "b"), "{changed:?}");
    assert_eq!(side_of(&map, "far"), Some(Side::Right));
}
