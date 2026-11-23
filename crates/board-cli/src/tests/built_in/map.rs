//! The map plugin through the binary: `board check` validates a map against
//! the schema `plugin enable` wrote, and `board map check`, the command the
//! skill names, lists what a reader would see as broken.

use super::*;

/// A map of two items 10 units apart, so their marks and their labels
/// overlap.
fn map() -> Value {
    let item = |id: &str, y: f64| json!({"id": id, "label": [format!("Item {id}")], "at": {"x": 40, "y": y, "set": "pinned"}});
    json!({
        "$type": "map/layout/v1",
        "title": "Two items",
        "sheet": {"width": 300, "height": 200},
        "items": [item("a", 60.0), item("b", 70.0)],
    })
}

#[test]
fn board_check_admits_a_map_and_marks_its_malformed_twin() {
    let board = Fixture::enabling(&["map"]);
    let mut twin = map();
    twin["items"][0]["side"] = json!("top");
    board
        .write("maps/good.json", &map().to_string())
        .write("maps/bad.json", &twin.to_string());
    let checked = board.checked("/maps/");
    assert_eq!(checked["/maps/good"], None);
    assert_eq!(
        checked["/maps/bad"].as_deref(),
        Some("board::record::invalid")
    );
}

#[test]
fn map_check_lists_the_overlap_and_its_apart_twin_has_none() {
    let board = Fixture::enabling(&["map"]);
    let mut apart = map();
    apart["items"][1]["at"]["y"] = json!(140);
    board
        .write("maps/close.json", &map().to_string())
        .write("maps/apart.json", &apart.to_string());
    let output = board.run(&["--json", "map", "check", "--path", "/maps/close"]);
    assert_eq!(output.code, 0, "{}", text(&output.stderr));
    let close = json(&output)["data"]["output"].clone();
    let found: Vec<(&str, &str)> = (close["findings"].as_array().unwrap().iter())
        .map(|finding| {
            (
                finding["kind"].as_str().unwrap(),
                finding["message"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        found,
        [
            ("items-overlap", "the marks of a and b overlap"),
            ("label-overlap", "the label of a overlaps the label of b"),
        ]
    );
    assert_eq!(close["findings"][1]["estimated"][1]["of"], "the label of b");
    let apart = board.call("map.check", &["--path", "/maps/apart"]);
    assert_eq!(apart["problems"], 0, "{apart}");
    let refused = board.refusal("map.check", &["--path", "/maps/none"]);
    assert_eq!(refused["code"], "map::layout::not_found", "{refused}");
}

/// A map that breaks its schema, such as an item with an empty label, is
/// refused as unreadable instead of being checked; its twin with a label is
/// checked.
#[test]
fn map_check_refuses_an_item_with_an_empty_label_and_checks_its_twin() {
    let board = Fixture::enabling(&["map"]);
    let mut empty = map();
    empty["items"][0]["label"] = json!([]);
    board
        .write("maps/empty.json", &empty.to_string())
        .write("maps/close.json", &map().to_string());
    let refused = board.refusal("map.check", &["--path", "/maps/empty"]);
    assert_eq!(refused["code"], "map::layout::unreadable", "{refused}");
    let checked = board.call("map.check", &["--path", "/maps/close"]);
    assert_eq!(checked["problems"], 2, "{checked}");
}
