//! The page's blocks are what the rules say, and they are the same whatever
//! order the inputs arrive in.

use serde_json::{Value, json};

use super::*;

/// Reverses every list and every object's key order. A pair inside a list is
/// a tuple and keeps its order.
fn reversed(value: &Value, in_list: bool) -> Value {
    match value {
        Value::Array(items) => {
            let mut items: Vec<Value> = items.iter().map(|item| reversed(item, true)).collect();
            if !in_list {
                items.reverse();
            }
            Value::Array(items)
        }
        Value::Object(map) => Value::Object(
            map.iter()
                .rev()
                .map(|(key, value)| (key.clone(), reversed(value, false)))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn pass() -> Value {
    serde_json::from_str(include_str!("../../page/fixture/pass.json")).unwrap()
}

fn facts() -> Value {
    serde_json::from_str(include_str!("../../page/fixture/facts.json")).unwrap()
}

/// The project of `page` with `id`.
fn project<'a>(page: &'a Page, id: &str) -> &'a Value {
    (page.data["projects"].as_array().unwrap().iter())
        .find(|project| project["id"] == id)
        .unwrap()
}

fn page(pass: &Value, facts: &Value) -> Result<Page, PluginError> {
    let pass = serde_json::from_value(pass.clone()).unwrap();
    let facts = serde_json::from_value(facts.clone()).unwrap();
    build(&pass, &facts)
}

#[test]
fn the_blocks_are_the_same_whatever_order_every_input_list_and_key_arrives_in() {
    let forward = page(&pass(), &facts()).unwrap();
    let backward = page(&reversed(&pass(), false), &reversed(&facts(), false)).unwrap();
    assert_eq!(forward, backward);
    assert_eq!(
        serde_json::to_string(&forward.data).unwrap(),
        serde_json::to_string(&backward.data).unwrap(),
        "the text is equal too, so the page's bytes are"
    );
}

#[test]
fn a_change_has_its_kinds_of_finding_its_repo_its_crates_and_no_more() {
    let page = page(&pass(), &facts()).unwrap();
    let speed = project(&page, "speed");
    let (bench, cache) = (&speed["changes"][0], &speed["changes"][1]);
    assert_eq!(
        (bench["id"].clone(), cache["id"].clone()),
        (json!("bench"), json!("cache"))
    );
    assert_eq!(cache["findings"], json!(["merge", "stale-plan"]));
    assert_eq!(cache["repo"], "core");
    assert_eq!(
        cache["crates"],
        json!(["demo-core"]),
        "demo-cored is no crate"
    );
    assert_eq!(bench["crates"], json!(["demo-base"]));
    assert_eq!(cache["tasks"], json!(["cache.md"]));
    assert_eq!(cache["date"], "2026-02-01");
    let guide = &project(&page, "docs")["changes"][0];
    assert_eq!(
        (guide["repo"].clone(), guide["date"].clone()),
        (json!("api"), Value::Null)
    );
    assert_eq!(guide["crates"], json!([]), "api has no workspace");
    let release = &project(&page, "shipped")["changes"][0];
    assert_eq!(
        (release["repo"].clone(), release["findings"].clone()),
        (Value::Null, json!(["landed"]))
    );
    assert_eq!(
        speed["order"],
        json!([["/changes/cache", "/changes/bench"]])
    );
}

#[test]
fn a_change_the_map_names_that_is_not_open_is_left_out_and_an_open_one_it_lacks_is_counted() {
    let page = page(&pass(), &facts()).unwrap();
    let ids: Vec<&str> = (project(&page, "speed")["changes"]
        .as_array()
        .unwrap()
        .iter())
    .map(|change| change["id"].as_str().unwrap())
    .collect();
    assert_eq!(
        ids,
        ["bench", "cache"],
        "gone is not open; extra is in no project"
    );
    assert_eq!(
        (page.data["open"].clone(), page.data["missing"].clone()),
        (json!(5), json!(1))
    );
}

#[test]
fn each_question_counts_on_one_row_or_on_its_realms_line_and_the_totals_add_up() {
    let page = page(&pass(), &facts()).unwrap();
    let by_project = &page.questions["projects"];
    assert_eq!(
        by_project["speed"][0]["text"],
        "Should cache wait for bench?"
    );
    assert_eq!(by_project["speed"][0]["by"], "change");
    assert_eq!(by_project["speed"][1]["by"], "task");
    assert_eq!(by_project["docs"][0]["by"], "name");
    let unassigned = &page.questions["unassigned"];
    assert_eq!(unassigned["alpha"][0]["from"], "notes");
    assert_eq!(unassigned["alpha"].as_array().unwrap().len(), 1);
    assert!(unassigned.get("beta").is_none(), "{unassigned}");
}

#[test]
fn a_realm_whose_rows_cannot_be_traced_to_its_total_is_refused_and_its_twin_is_admitted() {
    let mut twice = pass();
    twice["projects"].as_array_mut().unwrap().push(
        json!({"id": "docs", "realm": "alpha", "title": "Docs", "state": "queued", "task": null,
               "changes": []}),
    );
    let refused = page(&twice, &facts()).unwrap_err();
    assert_eq!(refused.code, "keeper::render::sums_disagree");
    page(&pass(), &facts()).unwrap();
}

#[test]
fn the_page_is_as_of_the_newest_day_on_any_input() {
    let page = page(&pass(), &facts()).unwrap();
    assert_eq!(page.questions["asOf"], "2026-02-04");
    let mut older = facts();
    older["changes"][4]["touched"] = json!("2026-01-01");
    let (pass, older) = (
        serde_json::from_value(pass()).unwrap(),
        serde_json::from_value(older).unwrap(),
    );
    assert_eq!(page_as_of(&pass, &older), "2026-02-03");
}

#[test]
fn systems_become_cities_links_and_external_names_and_workspaces_hold_their_crates_sorted() {
    let page = page(&pass(), &facts()).unwrap();
    let systems = &page.systems;
    assert_eq!(systems["cities"][0]["id"], "api");
    assert_eq!(
        systems["cities"][0]["districts"][0]["id"], "api.cli",
        "sorted by id"
    );
    assert_eq!(
        systems["external"],
        json!([{"id": "cloud", "title": "Cloud"}])
    );
    let (calls, events) = (&systems["links"][0], &systems["links"][1]);
    assert_eq!(
        (calls["from"].clone(), calls["kind"].clone()),
        (json!("api.server"), json!("road"))
    );
    assert_eq!(calls["symbols"][0]["at"], "to");
    assert_eq!(
        (events["from"].clone(), events["kind"].clone()),
        (json!("api"), json!("rail"))
    );
    let workspace = &page.data["workspaces"][0];
    assert_eq!(
        (workspace["id"].clone(), workspace["lang"].clone()),
        (json!("core"), json!("Rust"))
    );
    let crates = &workspace["crates"];
    assert_eq!(crates[1]["id"], "demo-core");
    assert_eq!(crates[1]["deps"], json!(["demo-base", "demo-util"]));
    assert_eq!(crates[1]["modules"][0]["id"], "lib");
}
