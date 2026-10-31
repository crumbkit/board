//! `thread` groups a request's versions by board, plugin, op and path,
//! orders them by `asked`, attaches answers and replies, and refuses an id
//! that names no effect.

use board_model::{Answer, Effect, Seen};
use board_ops::{Board, Plugins};
use board_plugin::Target;
use board_scenario::{Item, Scenario};
use serde_json::json;

use crate::Requests;
use serde_json::Value;

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
        .plugin_enable(None, None, &"requests".parse().unwrap())
        .unwrap();
    scenario
}

/// A decide by `review` of `review.comment` at `path`, showing `text`,
/// asked at `asked`.
fn effect(path: &str, text: &str, asked: Option<&str>) -> Effect {
    let mut effect = Effect::new(
        "/b".to_owned(),
        "review".parse().unwrap(),
        "review.comment".to_owned(),
        path.to_owned(),
    )
    .showing(None, Some(text.to_owned()));
    effect.asked = asked.map(str::to_owned);
    effect
}

/// Writes `effect` under `effects/<directory>/` and returns its id.
fn held(scenario: &Scenario, directory: &str, effect: &Effect) -> String {
    let id = effect.id();
    let text = serde_json::to_string(effect).unwrap();
    scenario.write(&format!("effects/{directory}/{id}.json"), Item::File(&text));
    id
}

fn answered(scenario: &Scenario, effect: &Effect, answer: Answer, reason: Option<&str>) -> String {
    let id = held(scenario, "answered", effect);
    let seen = Seen::new(
        format!("/effects/answered/{id}"),
        answer,
        reason.map(str::to_owned),
    );
    let text = serde_json::to_string(&seen).unwrap();
    scenario.write(&format!("effects/seen/{id}.json"), Item::File(&text));
    id
}

fn reply(scenario: &Scenario, id: &str, name: &str, text: &str) {
    let record = json!({"$type": "requests/reply/v1", "effect": id, "text": text});
    let path = format!("requests/replies/{id}/{name}.json");
    scenario.write(&path, Item::File(&record.to_string()));
}

/// `board call requests.thread --effect <id>` on `scenario`, in process:
/// its output, or the code it was refused with.
fn thread(scenario: &Scenario, id: &str) -> Result<Value, String> {
    let mut board = Board::open(scenario.workdir(), None).unwrap();
    board.build_in(&[&Requests]);
    let target: Target = "requests.thread".parse().unwrap();
    let input = json!({ "effect": id }).to_string();
    let called = board.call(&target, input.as_bytes(), &Plugins::default());
    let started = board.take_trace().work.get("started").copied();
    assert_eq!(started, Some(0), "a built-in call starts no process");
    called
        .map(|report| report.output)
        .map_err(|failed| failed.refusal().code.as_str().to_owned())
}

fn ids(output: &Value, key: &str) -> Vec<String> {
    (output[key].as_array().unwrap().iter())
        .map(|item| item["id"].as_str().unwrap_or_default().to_owned())
        .collect()
}

#[test]
fn two_pending_versions_and_a_reply_are_one_thread_with_the_newer_version_latest() {
    let scenario = personal();
    let first = held(
        &scenario,
        "pending",
        &effect("/x", "one\n", Some("2026-10-30T09:00:00.000000Z")),
    );
    let second = held(
        &scenario,
        "pending",
        &effect("/x", "two\n", Some("2026-10-30T10:00:00.000000Z")),
    );
    let elsewhere = held(
        &scenario,
        "pending",
        &effect("/y", "one\n", Some("2026-10-30T08:00:00.000000Z")),
    );
    reply(&scenario, &first, "20261030T093000Z", "Shorter, please.");
    reply(&scenario, &elsewhere, "20261030T083000Z", "Not this one.");
    for asked_by in [&first, &second] {
        let output = thread(&scenario, asked_by).unwrap();
        assert_eq!(ids(&output, "versions"), [first.clone(), second.clone()]);
        let versions = &output["versions"];
        assert_eq!(versions[0]["text"], "one\n");
        assert_eq!(
            (&versions[0]["pending"], &versions[0]["latest"]),
            (&json!(true), &json!(false))
        );
        assert_eq!(
            (&versions[1]["pending"], &versions[1]["latest"]),
            (&json!(true), &json!(true))
        );
        assert_eq!(versions[0].get("replaced_by"), None);
        assert_eq!(versions[1]["answer"], Value::Null);
        assert_eq!(
            output["replies"],
            json!([{
                "path": format!("/requests/replies/{first}/20261030T093000Z"),
                "effect": first,
                "text": "Shorter, please.",
            }])
        );
        assert_eq!(
            (&output["plugin"], &output["op"], &output["path"]),
            (&json!("review"), &json!("review.comment"), &json!("/x"))
        );
    }
}

#[test]
fn an_answered_version_carries_its_answer_and_an_effect_without_asked_comes_first() {
    let scenario = personal();
    let old = held(&scenario, "pending", &effect("/x", "zero\n", None));
    let refused = answered(
        &scenario,
        &effect("/x", "one\n", Some("2026-10-30T09:00:00.000000Z")),
        Answer::Refuse,
        Some("too long"),
    );
    let latest = held(
        &scenario,
        "pending",
        &effect("/x", "two\n", Some("2026-10-30T10:00:00.000000Z")),
    );
    reply(&scenario, &latest, "20261030T110000Z", "Better.");
    reply(&scenario, &refused, "20261030T093000Z", "Shorter, please.");
    let output = thread(&scenario, &refused).unwrap();
    assert_eq!(
        ids(&output, "versions"),
        [old.clone(), refused.clone(), latest.clone()]
    );
    let versions = &output["versions"];
    assert_eq!(versions[0]["asked"], Value::Null);
    assert_eq!(versions[2]["latest"], json!(true));
    assert_eq!(
        versions[1]["answer"],
        json!({"answer": "refuse", "reason": "too long"})
    );
    assert_eq!(
        (&versions[1]["pending"], &versions[1]["latest"]),
        (&json!(false), &json!(false))
    );
    let texts: Vec<&str> = (output["replies"].as_array().unwrap().iter())
        .map(|reply| reply["text"].as_str().unwrap())
        .collect();
    assert_eq!(texts, ["Shorter, please.", "Better."]);
}

#[test]
fn an_id_that_is_not_64_lowercase_hex_is_refused_as_not_found_and_a_pending_ones_is_read() {
    let scenario = personal();
    let id = held(&scenario, "pending", &effect("/x", "one\n", None));
    for malformed in [id.to_uppercase(), id[1..].to_owned(), String::new()] {
        let refused = thread(&scenario, &malformed).unwrap_err();
        assert_eq!(refused, "requests::effect::not_found", "{malformed}");
    }
    assert_eq!(ids(&thread(&scenario, &id).unwrap(), "versions"), [id]);
}

#[test]
fn an_id_that_names_only_a_seen_record_or_a_misnamed_effect_is_refused_as_not_found_and_its_answered_twin_is_read()
 {
    let scenario = personal();
    let one = effect("/x", "one\n", None);
    let id = one.id();
    let seen = Seen::new(format!("/effects/answered/{id}"), Answer::Refuse, None);
    let seen = serde_json::to_string(&seen).unwrap();
    scenario.write(&format!("effects/seen/{id}.json"), Item::File(&seen));
    assert_eq!(
        thread(&scenario, &id).unwrap_err(),
        "requests::effect::not_found"
    );
    let other = effect("/x", "two\n", None).id();
    let misnamed = serde_json::to_string(&one).unwrap();
    scenario.write(
        &format!("effects/answered/{other}.json"),
        Item::File(&misnamed),
    );
    assert_eq!(
        thread(&scenario, &other).unwrap_err(),
        "requests::effect::not_found"
    );
    held(&scenario, "answered", &one);
    let output = thread(&scenario, &id).unwrap();
    assert_eq!(ids(&output, "versions"), [id]);
    assert_eq!(output["versions"][0]["answer"]["answer"], "refuse");
}

#[test]
fn a_pending_effect_beside_an_answered_copy_of_the_same_id_is_one_pending_version() {
    let scenario = personal();
    let first = effect("/x", "one\n", Some("2026-10-30T09:00:00.000000Z"));
    let again = effect("/x", "one\n", Some("2026-10-30T12:00:00.000000Z"));
    let id = held(&scenario, "answered", &first);
    held(&scenario, "pending", &again);
    let output = thread(&scenario, &id).unwrap();
    assert_eq!(ids(&output, "versions"), [id]);
    let version = &output["versions"][0];
    assert_eq!(
        (&version["asked"], &version["pending"], &version["latest"]),
        (
            &json!("2026-10-30T12:00:00.000000Z"),
            &json!(true),
            &json!(true)
        )
    );
}

#[test]
fn a_pending_effect_the_owner_has_answered_no_longer_waits_and_is_not_latest() {
    let scenario = personal();
    let id = held(&scenario, "pending", &effect("/x", "one\n", None));
    let seen = Seen::new(format!("/effects/pending/{id}"), Answer::Approve, None);
    let seen = serde_json::to_string(&seen).unwrap();
    scenario.write(&format!("effects/seen/{id}.json"), Item::File(&seen));
    let output = thread(&scenario, &id).unwrap();
    let version = &output["versions"][0];
    assert_eq!(
        (
            &version["pending"],
            &version["latest"],
            &version["answer"]["answer"]
        ),
        (&json!(false), &json!(false), &json!("approve"))
    );
}

/// A reply counts only under `/requests/replies/<its effect>/`: one placed
/// elsewhere in the board, or in another effect's directory, joins no
/// thread, and its twin in its own directory does.
#[test]
fn a_reply_outside_its_effects_replies_directory_is_left_out_and_its_twin_inside_is_read() {
    let scenario = personal();
    let id = held(&scenario, "pending", &effect("/x", "one\n", None));
    let other = held(&scenario, "pending", &effect("/y", "one\n", None));
    let record = json!({"$type": "requests/reply/v1", "effect": id, "text": "Why?"});
    let record = record.to_string();
    for path in [
        "notes/20261030T093000Z.json".to_owned(),
        format!("requests/{id}/20261030T093000Z.json"),
        format!("requests/replies/{other}/20261030T093000Z.json"),
        format!("repos/requests/replies/{id}/20261030T093000Z.json"),
    ] {
        scenario.write(&path, Item::File(&record));
    }
    assert_eq!(thread(&scenario, &id).unwrap()["replies"], json!([]));
    reply(&scenario, &id, "20261030T093000Z", "Why?");
    let replies = &thread(&scenario, &id).unwrap()["replies"];
    assert_eq!(
        replies,
        &json!([{
            "path": format!("/requests/replies/{id}/20261030T093000Z"),
            "effect": id,
            "text": "Why?",
        }])
    );
}
