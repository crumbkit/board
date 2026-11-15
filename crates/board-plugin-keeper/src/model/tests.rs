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
    json!({
        "date": "2026-02-03",
        "realms": [{"id": "alpha", "title": "Alpha"}, {"id": "beta", "title": "Beta"}],
        "projects": [
            {"id": "speed", "realm": "alpha", "title": "Speed", "state": "active", "task": null,
             "changes": ["/changes/cache", "/changes/bench", "/changes/gone"],
             "order": [["/changes/cache", "/changes/bench"]]},
            {"id": "docs", "realm": "beta", "title": "Docs", "state": "queued", "task": null,
             "changes": ["/changes/guide"]},
        ],
        "findings": [
            {"kind": "stale-plan", "paths": ["/changes/cache"], "evidence": "Old.", "fix": "x",
             "id": "stale-plan-00000001"},
            {"kind": "merge", "paths": ["/changes/cache", "/changes/bench"], "evidence": "Same.",
             "fix": "x", "id": "merge-00000002"},
            {"kind": "stale", "paths": ["/changes/guide"], "evidence": "Idle.", "fix": "x",
             "id": "stale-00000003"},
        ],
        "links": [{"before": "/changes/bench", "after": "/changes/guide", "project": "speed"}],
        "systems": [
            {"id": "api", "realm": "alpha", "title": "API", "language": "Rust",
             "modules": [{"id": "server", "lines": 900, "purpose": "Serves."},
                         {"id": "cli", "lines": 100, "purpose": "Commands."}],
             "links": [{"id": "calls", "name": "Calls", "module": "server", "to": "cloud",
                        "kind": "api-call", "traffic": 4, "production": true,
                        "controls": [{"caption": "Gate", "detail": "Token.", "at": "to"}]}]},
            {"id": "cloud", "realm": "alpha", "title": "Cloud", "external": true},
        ],
        "question_rules": {
            "mount_realms": {"team": "beta"}, "localRealm": {"notes": "alpha"},
            "localDefault": "beta",
            "tasks": [{"contains": "profile-task", "to": "speed"}],
            "names": {"guide": "docs"},
        },
    })
}

fn facts() -> Value {
    json!({
        "changes": [
            {"path": "/changes/cache", "title": "Cache results", "phase": "build",
             "mount": "/repos/api", "touched": "2026-02-01", "task": "/tasks/cache.md",
             "plan": "Edits server and the crate api-core, not api-cored."},
            {"path": "/changes/bench", "title": "Benchmark", "phase": "plan",
             "mount": "/repos/api", "touched": "2026-02-02", "task": null, "plan": null},
            {"path": "/changes/guide", "title": "Write the guide", "phase": "review",
             "mount": null, "touched": null, "task": null, "plan": null},
            {"path": "/changes/extra", "title": "Not in the map", "phase": "plan",
             "mount": null, "touched": "2026-02-04", "task": null, "plan": null},
        ],
        "questions": [
            {"path": "/projects/p/questions/one", "asked": "2026-02-02",
             "text": "Should cache wait for bench?", "blocked_work": ""},
            {"path": "/projects/guide/questions/two", "asked": "2026-02-01",
             "text": "Which style?", "blocked_work": ""},
            {"path": "/projects/notes/questions/three", "asked": "2026-02-03",
             "text": "Anything?", "blocked_work": ""},
            {"path": "/repos/team/projects/misc/questions/four", "asked": "2026-02-03",
             "text": "Other?", "blocked_work": "profile-task"},
        ],
        "crates": [
            {"id": "api-core", "lines": 700, "deps": ["api-util", "api-base"],
             "modules": [{"id": "z", "lines": 1}, {"id": "a", "lines": 2}]},
            {"id": "api-util", "lines": 300, "deps": [], "modules": []},
        ],
    })
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
    let speed = &page.data["projects"][1];
    assert_eq!(speed["id"], "speed");
    let cache = &speed["changes"][1];
    assert_eq!(cache["id"], "cache");
    assert_eq!(cache["findings"], json!(["merge", "stale-plan"]));
    assert_eq!(cache["repo"], "api");
    assert_eq!(
        cache["crates"],
        json!(["api-core"]),
        "api-cored is no crate"
    );
    assert_eq!(cache["tasks"], json!(["cache.md"]));
    assert_eq!(cache["date"], "2026-02-01");
    let guide = &page.data["projects"][0]["changes"][0];
    assert_eq!(
        (guide["repo"].clone(), guide["date"].clone()),
        (Value::Null, Value::Null)
    );
    assert_eq!(
        page.data["projects"][1]["order"],
        json!([["/changes/cache", "/changes/bench"]])
    );
}

#[test]
fn a_change_the_map_names_that_is_not_open_is_left_out_and_an_open_one_it_lacks_is_counted() {
    let page = page(&pass(), &facts()).unwrap();
    let ids: Vec<&str> = (page.data["projects"][1]["changes"]
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
        (json!(4), json!(1))
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
    older["changes"][3]["touched"] = json!("2026-01-01");
    let (pass, older) = (
        serde_json::from_value(pass()).unwrap(),
        serde_json::from_value(older).unwrap(),
    );
    assert_eq!(page_as_of(&pass, &older), "2026-02-03");
}

#[test]
fn systems_become_cities_links_and_external_names_sorted_by_id() {
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
    let link = &systems["links"][0];
    assert_eq!(
        (link["from"].clone(), link["kind"].clone()),
        (json!("api.server"), json!("road"))
    );
    assert_eq!(link["symbols"][0]["at"], "to");
    let crates = &page.data["crates"];
    assert_eq!(crates[0]["deps"], json!(["api-base", "api-util"]));
    assert_eq!(crates[0]["modules"][0]["id"], "a");
}
