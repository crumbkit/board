//! `threads` lists the thread of every request with a pending version, each
//! as `thread` returns it, in board, plugin, op and path order, and reads
//! the effects and the replies once each.

use board_model::{Answer, CapabilityName, Effect, Seen};
use board_ops::{Board, Plugin, Plugins};
use board_plugin::Target;
use board_scenario::{Item, Scenario};
use serde_json::{Value, json};

use crate::Requests;
use crate::tests::empty;

/// A personal board that enables requests, built in.
fn personal() -> Scenario {
    let scenario = Scenario::new();
    scenario.write(
        ".board/board.json",
        Item::File(r#"{"$type": "board/config/v1", "commit_on_put": false}"#),
    );
    let mut board = Board::open(scenario.workdir(), None).unwrap();
    board.build_in(&[&Requests]);
    board
        .plugin_enable(None, None, &"requests".parse().unwrap(), &[])
        .unwrap();
    scenario
}

/// A decide by `plugin` of `review.comment` at `path`, showing `text`,
/// asked at `asked`.
fn effect(plugin: &str, path: &str, text: &str, asked: &str) -> Effect {
    let mut effect = Effect::new(
        "/b".to_owned(),
        plugin.parse().unwrap(),
        "review.comment".to_owned(),
        path.to_owned(),
    )
    .showing(None, Some(text.to_owned()));
    effect.asked = Some(asked.to_owned());
    effect
}

/// Writes `effect` under `effects/<directory>/` and returns its id.
fn held(scenario: &Scenario, directory: &str, effect: &Effect) -> String {
    let id = effect.id();
    let text = serde_json::to_string(effect).unwrap();
    scenario.write(&format!("effects/{directory}/{id}.json"), Item::File(&text));
    id
}

fn answered(scenario: &Scenario, effect: &Effect) -> String {
    let id = held(scenario, "answered", effect);
    let seen = Seen::new(&id, Answer::Approve, None);
    let text = serde_json::to_string(&seen).unwrap();
    scenario.write(&format!("effects/seen/{id}.json"), Item::File(&text));
    id
}

fn reply(scenario: &Scenario, id: &str, name: &str, text: &str) {
    let record =
        json!({"$type": "requests/reply/v1", "effect": format!("sha256:{id}"), "text": text});
    let path = format!("requests/replies/{id}/{name}.json");
    scenario.write(&path, Item::File(&record.to_string()));
}

/// `board call requests.<capability>` with `input` on `scenario`, in
/// process: its output, or the code it was refused with, and how many
/// queries it ran.
fn call(scenario: &Scenario, capability: &str, input: &Value) -> (Result<Value, String>, usize) {
    let mut board = Board::open(scenario.workdir(), None).unwrap();
    board.build_in(&[&Requests]);
    let target: Target = format!("requests.{capability}").parse().unwrap();
    let input = input.to_string();
    let called = board.call(&target, input.as_bytes(), &Plugins::default());
    let trace = board.take_trace();
    let started = trace.work.get("started").copied();
    assert_eq!(started, Some(0), "a built-in call starts no process");
    let queries = (trace.phases.iter())
        .find(|phase| phase.name == "query.sql")
        .map_or(0, |phase| phase.entered);
    let called = called
        .map(|report| report.output)
        .map_err(|failed| failed.refusal().code.as_str().to_owned());
    (called, queries)
}

fn threads(scenario: &Scenario) -> (Value, usize) {
    let (output, queries) = call(scenario, "threads", &json!({}));
    (output.unwrap(), queries)
}

fn ids(thread: &Value) -> Vec<String> {
    (thread["versions"].as_array().unwrap().iter())
        .map(|version| version["id"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn every_request_with_a_pending_version_is_one_thread_in_order_as_thread_returns_it() {
    let scenario = personal();
    // Listed second: `review` sorts after `notes`.
    let first = held(
        &scenario,
        "pending",
        &effect("review", "/x", "one\n", "2026-10-30T09:00:00.000000Z"),
    );
    let second = held(
        &scenario,
        "pending",
        &effect("review", "/x", "two\n", "2026-10-30T10:00:00.000000Z"),
    );
    reply(&scenario, &first, "20261030T093000Z", "Shorter, please.");
    // Listed first.
    let alone = held(
        &scenario,
        "pending",
        &effect("notes", "/z", "one\n", "2026-10-30T11:00:00.000000Z"),
    );
    // Not listed: answered, with a reply.
    let done = answered(
        &scenario,
        &effect("review", "/y", "one\n", "2026-10-30T08:00:00.000000Z"),
    );
    reply(&scenario, &done, "20261030T083000Z", "Thanks.");

    let (output, queries) = threads(&scenario);
    assert_eq!(queries, 2, "one effects query and one replies query");
    let listed = output["threads"].as_array().unwrap();
    let versions: Vec<Vec<String>> = listed.iter().map(ids).collect();
    assert_eq!(
        versions,
        [vec![alone.clone()], vec![first.clone(), second.clone()]]
    );
    for (thread, pending) in listed.iter().zip([vec![alone], vec![first, second]]) {
        for id in pending {
            let (alone, _) = call(&scenario, "thread", &json!({ "effect": id }));
            assert_eq!(thread, &alone.unwrap(), "{id}");
        }
    }
    assert!(!output.to_string().contains(&done), "{output}");
    assert!(!output.to_string().contains("Thanks."), "{output}");
}

#[test]
fn a_board_with_no_pending_effect_lists_no_thread_and_reads_no_replies() {
    let scenario = personal();
    let done = answered(
        &scenario,
        &effect("review", "/y", "one\n", "2026-10-30T08:00:00.000000Z"),
    );
    reply(&scenario, &done, "20261030T083000Z", "Thanks.");
    let (output, queries) = threads(&scenario);
    assert_eq!(output, json!({"threads": []}));
    assert_eq!(queries, 1, "the effects query alone");
}

/// Board's schema check refuses an input property before the plugin reads
/// it, and the plugin refuses one that reaches it.
#[test]
fn an_input_property_is_refused_and_an_empty_input_is_read() {
    let scenario = personal();
    let input = json!({"effect": "a".repeat(64)});
    let (refused, _) = call(&scenario, "threads", &input);
    assert_eq!(refused.unwrap_err(), "board::plugin::bad_input");
    let capability: CapabilityName = "threads".parse().unwrap();
    let refused = Requests.call(&empty(), &capability, input).unwrap_err();
    assert_eq!(refused.code, "requests::call::bad_input");
    let read = Requests.call(&empty(), &capability, json!({})).unwrap();
    assert_eq!(read, json!({"threads": []}));
    let (output, _) = threads(&scenario);
    assert_eq!(output, json!({"threads": []}));
}
