//! The page draws every part of a map, makes no network request, escapes
//! text, and refuses a map whose bins, tables or line kinds do not fit.

use serde_json::{Value, json};

use super::*;
use crate::layout_schema;
use crate::tests::{board_with, called_as, fixture};

/// The queue fixture with a ramp, a shade for each item, a style for its
/// line kind and two tables, one with a row for each item.
fn shaded() -> Value {
    let mut map = fixture("queue-final");
    map["ramp"] = json!({"label": "Waits on", "bins": ["nothing", "1", "2", "3", "4+"]});
    let ids: Vec<String> = (map["items"].as_array().unwrap().iter())
        .map(|item| item["id"].as_str().unwrap().to_owned())
        .collect();
    for (at, item) in map["items"].as_array_mut().unwrap().iter_mut().enumerate() {
        item["bin"] = json!(at % 5);
    }
    let kind = map["lines"][0]["kind"].as_str().unwrap().to_owned();
    map["line_kinds"] = json!([{"kind": kind, "label": "builds after", "style": "dashed"}]);
    let rows: Vec<Value> = ids
        .iter()
        .map(|id| json!({"item": id, "cells": [id, "3"]}))
        .collect();
    map["tables"] = json!([
        {"id": "every-item", "title": "Every item",
         "columns": [{"label": "Item"}, {"label": "Waits on", "numeric": true}],
         "rows": rows, "caption": "Waits on counts what is unfinished."}
    ]);
    map
}

fn read(value: &Value) -> LayoutRecord {
    serde_json::from_value(value.clone()).unwrap()
}

#[test]
fn the_page_draws_each_part_of_the_map_and_makes_no_network_request() {
    let value = shaded();
    let html = render(&read(&value));
    for item in value["items"].as_array().unwrap() {
        assert!(
            html.contains(&format!("data-id=\"{}\"", item["id"].as_str().unwrap())),
            "{item}"
        );
        assert!(html.contains(item["label"][0].as_str().unwrap()));
    }
    for region in value["regions"].as_array().unwrap() {
        assert!(html.contains(region["title"].as_str().unwrap()));
    }
    for text in [
        "Waits on",
        "nothing",
        "4+",
        "Every item",
        "builds after",
        "stroke-dasharray=\"6 4\"",
    ] {
        assert!(html.contains(text), "{text}");
    }
    assert_eq!(
        html.matches("<circle").count(),
        value["items"].as_array().unwrap().len()
    );
    assert_eq!(
        html.matches("class=\"edge\"").count(),
        value["lines"].as_array().unwrap().len()
    );
    assert_eq!(
        html.matches("class=\"note\"").count(),
        value["notes"].as_array().unwrap().len()
    );
    for network in [
        "http:", "https:", "href=", "src=", "@import", "url(\"", "url(http", "<link", "<img",
    ] {
        assert!(!html.contains(network), "{network}");
    }
}

#[test]
fn the_page_escapes_the_text_of_a_map() {
    let mut value = shaded();
    value["title"] = json!("Maps <script>alert(1)</script> & \"more\"");
    let html = render(&read(&value));
    assert!(html.contains("Maps &lt;script&gt;alert(1)&lt;/script&gt; &amp; &quot;more&quot;"));
    assert!(!html.contains("<script>alert"));
    let twin = render(&read(&shaded()));
    assert_eq!(twin.matches("<script>").count(), 1);
}

#[test]
fn a_line_kind_with_no_arrow_ends_without_one_and_its_twin_ends_with_one() {
    let mut value = shaded();
    let arrows = |value: &Value| render(&read(value)).matches("marker-end").count();
    let with = arrows(&value);
    value["line_kinds"][0]["arrow"] = json!(false);
    assert_eq!(arrows(&value), 0);
    assert_eq!(with, value["lines"].as_array().unwrap().len());
}

#[test]
fn the_page_leaves_out_items_with_no_position_and_names_them() {
    let (_scenario, mut board) = board_with(&{
        let mut value = shaded();
        value["items"][0].as_object_mut().unwrap().remove("at");
        value
    });
    let output = called_as(&mut board, "map.page", &json!({"path": "/maps/m"})).unwrap();
    let id = shaded()["items"][0]["id"].as_str().unwrap().to_owned();
    assert_eq!(output["unplaced"], json!([id.clone()]));
    assert!(
        !output["html"]
            .as_str()
            .unwrap()
            .contains(&format!("class=\"node\" data-id=\"{id}\""))
    );
}

#[test]
fn map_page_returns_the_page_and_refuses_what_is_not_a_map() {
    let (_scenario, mut board) = board_with(&shaded());
    let output = called_as(&mut board, "map.page", &json!({"path": "/maps/m"})).unwrap();
    assert!(
        output["html"]
            .as_str()
            .unwrap()
            .starts_with("<!doctype html>")
    );
    for (path, code) in [
        ("/maps/none", "map::layout::not_found"),
        ("/.board/board", "map::layout::unreadable"),
    ] {
        assert_eq!(
            called_as(&mut board, "map.page", &json!({"path": path})),
            Err(code.to_owned()),
            "{path}"
        );
    }
}

/// Each way a ramp, a table or a line kind can be malformed is refused with
/// its code, beside the admitted twin that the shaded map is.
#[test]
fn a_map_whose_bins_tables_or_line_kinds_do_not_fit_is_refused_and_its_twin_is_admitted() {
    assert!(check(&read(&shaded())).is_ok());
    let cases: [(&str, Value, &str); 4] = [
        ("/items/0/bin", json!(5), "map::layout::bin_out_of_range"),
        (
            "/ramp/bins",
            json!(["a", "b"]),
            "map::layout::bin_out_of_range",
        ),
        (
            "/tables/0/rows/0/cells",
            json!(["only one"]),
            "map::layout::table_shape",
        ),
        (
            "/tables/0/rows/0/item",
            json!("nobody"),
            "map::layout::unknown_id",
        ),
    ];
    for (pointer, value, code) in cases {
        let mut twin = shaded();
        *twin.pointer_mut(pointer).unwrap() = value;
        let refused = check(&read(&twin)).map(|_| ()).unwrap_err();
        assert_eq!(refused.code, code, "{pointer}");
    }
    let mut unramped = shaded();
    unramped.as_object_mut().unwrap().remove("ramp");
    assert_eq!(
        check(&read(&unramped)).unwrap_err().code,
        "map::layout::bin_out_of_range"
    );
    let mut twice = shaded();
    let table = twice["tables"][0].clone();
    twice["tables"].as_array_mut().unwrap().push(table);
    assert_eq!(
        check(&read(&twice)).unwrap_err().code,
        "map::layout::duplicate_id"
    );
    let mut kinds = shaded();
    let kind = kinds["line_kinds"][0].clone();
    kinds["line_kinds"].as_array_mut().unwrap().push(kind);
    assert_eq!(
        check(&read(&kinds)).unwrap_err().code,
        "map::layout::duplicate_id"
    );
}

/// The schema admits the shaded map and refuses each malformed field, and
/// the older fixtures, which have none of these fields, stay admitted.
#[test]
fn the_schema_admits_the_shaded_map_and_refuses_its_malformed_twins() {
    let schema = board_validate::Schema::compile(layout_schema()).unwrap();
    schema.validate(&shaded()).unwrap();
    for name in ["services-final", "work-final", "queue-placed"] {
        schema.validate(&fixture(name)).unwrap();
    }
    let refused: [(&str, Value); 9] = [
        ("/ramp/bins", json!(["only"])),
        ("/ramp/bins", json!(["a", "b", "c", "d", "e", "f"])),
        ("/ramp/label", json!("")),
        ("/items/0/bin", json!(-1)),
        ("/items/0/bin", json!(5)),
        ("/line_kinds/0/style", json!("wavy")),
        ("/tables/0/columns", json!([])),
        ("/tables/0/id", json!("Not A Slug")),
        ("/line_kinds", json!([])),
    ];
    for (pointer, value) in refused {
        let mut twin = shaded();
        *twin.pointer_mut(pointer).unwrap() = value;
        assert!(schema.validate(&twin).is_err(), "{pointer}");
    }
}
