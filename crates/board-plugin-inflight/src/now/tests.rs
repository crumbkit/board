//! `inflight.now` over a real board, in process. The cases were the pytest
//! suite of crumbkit/board#62, less the agents, claims and holders that
//! board does not record.
//!
//! changes is still a Python plugin, so these tests build in a stand-in
//! named changes. It declares the real plugin's `open` and `next` with their
//! schemas, so Board checks inflight's calls and the stand-in's answers
//! against them, and it answers from the change records with a fixed gate.

use board_model::{CapabilityName, RecordType};
use board_ops::{Board, Limit, Plugin, Plugins};
use board_plugin::{Manifest, Target};
use board_plugin_artifacts::Artifacts;
use board_plugin_evals::Evals;
use board_plugin_plans::Plans;
use board_scenario::{Item, Scenario};
use serde_json::{Value, json};

use super::*;
use crate::Inflight;

const PR: &str = "https://github.com/example/app/pull/7";

/// The real changes plugin's manifest, whose `open` and `next` the
/// stand-in declares.
const CHANGES: &str = include_str!("../../../../plugins/changes/plugin.json");
const CHANGE_SCHEMA: &str =
    include_str!("../../../../plugins/changes/types/changes/change/v1.schema.json");

/// A stand-in for the changes plugin with its `open` and `next`.
struct Changes;

impl Plugin for Changes {
    fn manifest(&self) -> Manifest {
        let mut manifest: Value = serde_json::from_str(CHANGES).unwrap();
        manifest.as_object_mut().unwrap().remove("run");
        let capabilities = manifest["capabilities"].as_array_mut().unwrap();
        capabilities
            .retain(|capability| ["open", "next"].contains(&capability["name"].as_str().unwrap()));
        manifest["types"] = json!(["changes/change/v1"]);
        Manifest::built_in(manifest).unwrap()
    }

    fn schemas(&self) -> Vec<(RecordType, Value)> {
        let schema = serde_json::from_str(CHANGE_SCHEMA).unwrap();
        vec![("changes/change/v1".parse().unwrap(), schema)]
    }

    fn call(
        &self,
        host: &dyn Host,
        capability: &CapabilityName,
        input: Value,
    ) -> Result<Value, PluginError> {
        let sql =
            "SELECT logical, body FROM records WHERE type = 'changes/change/v1' ORDER BY logical";
        let rows = host.query(sql, Limit::ALL).unwrap().rows;
        let changes = (rows.into_iter()).map(|row| {
            let body: Value = serde_json::from_str(row[1].as_str().unwrap()).unwrap();
            (row[0].as_str().unwrap().to_owned(), body)
        });
        let open: Vec<(String, Value)> = changes
            .filter(|(_, change)| !["done", "dropped"].contains(&change["phase"].as_str().unwrap()))
            .collect();
        if capability.as_str() == "open" {
            let limit = usize::try_from(input["limit"].as_u64().unwrap_or(100)).unwrap();
            let listed: Vec<Value> = (open.iter().take(limit))
                .map(|(path, change)| {
                    json!({
                        "path": path, "title": change["title"], "mount": change["mount"],
                        "phase": change["phase"], "route": change.get("route"),
                        "task": change.get("task"), "kind": "build", "prototype": null,
                    })
                })
                .collect();
            return Ok(json!({
                "truncated": open.len() > listed.len(),
                "changes": listed, "total": open.len(), "unreadable": 0,
            }));
        }
        let path = input["path"].as_str().unwrap();
        let (_, change) = open.iter().find(|(open, _)| open == path).unwrap();
        if change["mount"] == "/repos/gone" {
            return Err(PluginError {
                code: "changes::mount::not_found".to_owned(),
                message: "the mount /repos/gone holds nothing".to_owned(),
                fields: serde_json::Map::new(),
                hint: None,
            });
        }
        Ok(json!({
            "path": path, "phase": change["phase"], "route": change.get("route"),
            "gate": [{"check": "plan", "status": "pass", "detail": "written"}],
            "instruction": format!("Take it through {}.", change["phase"].as_str().unwrap()),
        }))
    }
}

/// A board that builds in inflight, the changes stand-in, artifacts, plans
/// and evals, and enables each of `enabled`.
fn board(enabled: &[&str]) -> Scenario {
    let scenario = Scenario::new();
    let config = r#"{"$type": "board/config/v1", "commit_on_put": false}"#;
    scenario.write(".board/board.json", Item::File(config));
    let board = opened(&scenario);
    for name in std::iter::once(&"inflight").chain(enabled) {
        (board.plugin_enable(None, None, &name.parse().unwrap(), &[])).unwrap();
    }
    scenario
}

fn opened(scenario: &Scenario) -> Board {
    let mut board = Board::open(scenario.workdir(), None).unwrap();
    board.build_in(&[&Inflight, &Changes, &Artifacts, &Plans, &Evals]);
    board
}

const EVERY: &[&str] = &["changes", "artifacts", "plans", "evals"];

fn write(scenario: &Scenario, path: &str, record: &Value) {
    scenario.write(path, Item::File(&record.to_string()));
}

fn change(scenario: &Scenario, name: &str, phase: &str, more: &Value) {
    let mut record = json!({
        "$type": "changes/change/v1", "title": format!("Change {name}"),
        "mount": "/repos/app", "phase": phase, "route": "owner",
    });
    record
        .as_object_mut()
        .unwrap()
        .extend(more.as_object().unwrap().clone());
    write(scenario, &format!("changes/{name}.json"), &record);
}

fn artifact(scenario: &Scenario, name: &str, subject: &str, title: &str, at: &str) {
    write(
        scenario,
        &format!("artifacts/{name}.json"),
        &json!({
            "$type": "artifacts/artifact/v1", "url": format!("https://claude.ai/artifact/{name}"),
            "account": "owner@example.com", "first_seen": at, "state": "draft",
            "subject": [subject],
        }),
    );
    write(
        scenario,
        &format!("artifacts/{name}/versions/1793473476-1c42.json"),
        &json!({
            "$type": "artifacts/version/v1", "version": "1793473476-1c42", "title": title,
            "sha256": "0".repeat(64), "source": "/work/plan.html", "session": "s2", "at": at,
        }),
    );
}

fn eval(scenario: &Scenario, name: &str) {
    write(
        scenario,
        &format!("evals/{name}.json"),
        &json!({"$type": "evals/eval/v1", "question": "Is it fast?", "subjects": ["app"], "status": "planned"}),
    );
}

/// Four open changes at different phases, a done one, a waiting question,
/// an open eval and an artifact of the change in review.
fn stopped() -> Scenario {
    let scenario = board(EVERY);
    write(
        &scenario,
        "repos/app.json",
        &json!({"$type": "board/repo/v1", "remote": "example/app"}),
    );
    change(&scenario, "a", "plan", &json!({}));
    change(&scenario, "b", "build", &json!({}));
    change(&scenario, "c", "review", &json!({"pull_request": PR}));
    change(&scenario, "d", "land", &json!({"route": "ask-first"}));
    change(&scenario, "e", "done", &json!({}));
    write(
        &scenario,
        "projects/p/questions/q1.json",
        &json!({
            "$type": "plans/question/v1", "question": "Go ahead with d?",
            "session": "local_0002", "asked": "2026-11-01T08:10:00Z", "status": "open",
        }),
    );
    eval(&scenario, "e");
    artifact(
        &scenario,
        "x",
        "/changes/c",
        "Plan of c",
        "2026-11-01T08:20:00Z",
    );
    scenario
}

/// The output of `board call inflight.now` with `input`, in process, and
/// that it started no process.
fn now(scenario: &Scenario, input: &Value) -> Value {
    called(scenario, input).unwrap()
}

fn called(scenario: &Scenario, input: &Value) -> Result<Value, String> {
    let board = opened(scenario);
    let target: Target = "inflight.now".parse().unwrap();
    let report = board.call(&target, input.to_string().as_bytes(), &Plugins::default());
    let started = board.take_trace().work.get("started").copied();
    assert_eq!(started.unwrap_or(0), 0, "a built-in call starts no process");
    (report.map(|report| report.output)).map_err(|failed| {
        let refusal = failed.refusal();
        format!("{}: {}", refusal.code.as_str(), refusal.message)
    })
}

fn by_path(output: &Value) -> Vec<(String, Value)> {
    (output["changes"]["changes"].as_array().unwrap().iter())
        .map(|entry| (entry["path"].as_str().unwrap().to_owned(), entry.clone()))
        .collect()
}

fn paths(entries: &Value) -> Vec<&str> {
    (entries.as_array().unwrap().iter())
        .map(|entry| entry["path"].as_str().unwrap())
        .collect()
}

#[test]
fn now_lists_each_open_change_with_its_phase_route_gate_and_instruction() {
    let output = now(&stopped(), &json!({}));
    let listed: Vec<Vec<String>> = (by_path(&output).into_iter())
        .map(|(path, entry)| {
            let field = |name: &str| entry[name].as_str().unwrap().to_owned();
            vec![path, field("phase"), field("route"), field("instruction")]
        })
        .collect();
    assert_eq!(
        listed,
        [
            ["/changes/a", "plan", "owner", "Take it through plan."],
            ["/changes/b", "build", "owner", "Take it through build."],
            ["/changes/c", "review", "owner", "Take it through review."],
            ["/changes/d", "land", "ask-first", "Take it through land."],
        ]
    );
    let a = &by_path(&output)[0].1;
    assert_eq!(
        a["gate"],
        json!([{"check": "plan", "status": "pass", "detail": "written"}])
    );
    assert_eq!(
        (&a["title"], &a["mount"], &a["kind"]),
        (&json!("Change a"), &json!("/repos/app"), &json!("build"))
    );
    assert_eq!(a["refused"], json!([]));
    let changes = &output["changes"];
    assert_eq!(
        [
            &changes["total"],
            &changes["left_out"],
            &changes["unreadable"],
            &changes["refused"]
        ],
        [&json!(4), &json!(0), &json!(0), &json!(null)]
    );
}

#[test]
fn now_shows_the_pull_request_with_the_command_that_checks_it_and_the_artifacts_of_the_change() {
    let output = now(&stopped(), &json!({}));
    let changes = by_path(&output);
    let (a, c) = (&changes[0].1, &changes[2].1);
    assert_eq!(c["pull_request"], PR);
    assert_eq!(c["pr_state"], format!("gh pr view {PR} --json state"));
    assert_eq!(
        (&a["pull_request"], &a["pr_state"]),
        (&json!(null), &json!(null))
    );
    assert_eq!(
        c["artifacts"],
        json!([{
            "path": "/artifacts/x", "url": "https://claude.ai/artifact/x", "state": "draft",
            "title": "Plan of c", "at": "2026-11-01T08:20:00Z",
        }])
    );
    assert_eq!(
        (&c["artifacts_left_out"], &a["artifacts"]),
        (&json!(0), &json!([]))
    );
}

#[test]
fn now_lists_the_waiting_question_and_the_open_eval_as_their_plugins_give_them() {
    let output = now(&stopped(), &json!({}));
    let questions = &output["questions"];
    assert_eq!(paths(&questions["entries"]), ["/projects/p/questions/q1"]);
    assert_eq!(questions["entries"][0]["question"], "Go ahead with d?");
    assert_eq!(
        [
            &questions["total"],
            &questions["left_out"],
            &questions["unreadable"],
            &questions["refused"]
        ],
        [&json!(1), &json!(0), &json!(0), &json!(null)]
    );
    assert_eq!(paths(&output["evals"]["entries"]), ["/evals/e"]);
    assert_eq!(output["evals"]["refused"], json!(null));
}

#[test]
fn limit_bounds_each_part_and_counts_what_it_left_out() {
    let scenario = stopped();
    eval(&scenario, "f");
    artifact(
        &scenario,
        "y",
        "/changes/a",
        "Older plan of a",
        "2026-11-01T07:00:00Z",
    );
    artifact(
        &scenario,
        "z",
        "/changes/a",
        "Plan of a",
        "2026-11-01T08:00:00Z",
    );
    let output = now(&scenario, &json!({"limit": 1}));
    let changes = by_path(&output);
    assert_eq!(changes.len(), 1);
    assert_eq!(paths(&changes[0].1["artifacts"]), ["/artifacts/z"]);
    assert_eq!(changes[0].1["artifacts_left_out"], 1);
    assert_eq!(
        (&output["changes"]["total"], &output["changes"]["left_out"]),
        (&json!(4), &json!(3))
    );
    assert_eq!(paths(&output["evals"]["entries"]), ["/evals/e"]);
    assert_eq!(
        (&output["evals"]["total"], &output["evals"]["left_out"]),
        (&json!(2), &json!(1))
    );
}

#[test]
fn a_limit_outside_1_to_50_is_refused_and_50_is_admitted() {
    let scenario = stopped();
    for refused in [0, 51] {
        assert_eq!(
            called(&scenario, &json!({"limit": refused})),
            Err("board::plugin::bad_input".to_owned())
        );
    }
    assert_eq!(now(&scenario, &json!({"limit": 50}))["changes"]["total"], 4);
}

#[test]
fn a_change_whose_next_step_is_refused_says_why_and_the_rest_are_still_listed() {
    let scenario = stopped();
    change(&scenario, "a", "plan", &json!({"mount": "/repos/gone"}));
    let changes = by_path(&now(&scenario, &json!({})));
    let (a, b) = (&changes[0].1, &changes[1].1);
    assert_eq!(
        (&a["gate"], &a["instruction"]),
        (&json!(null), &json!(null))
    );
    assert_eq!(a["refused"][0]["capability"], "changes.next");
    assert_eq!(a["refused"][0]["code"], "changes::mount::not_found");
    assert_eq!(
        (&b["instruction"], &b["refused"]),
        (&json!("Take it through build."), &json!([]))
    );
}

#[test]
fn a_part_whose_plugin_is_not_enabled_is_empty_and_says_why_while_the_others_are_listed() {
    let scenario = board(&["changes", "artifacts", "plans"]);
    change(&scenario, "a", "plan", &json!({}));
    let output = now(&scenario, &json!({}));
    let evals = &output["evals"];
    assert_eq!(
        (&evals["entries"], &evals["total"]),
        (&json!([]), &json!(0))
    );
    assert_eq!(evals["refused"]["capability"], "evals.open");
    let code = evals["refused"]["code"].as_str().unwrap();
    assert!(code.starts_with("board::plugin::"), "{code}");
    assert_eq!(paths(&output["changes"]["changes"]), ["/changes/a"]);
    let enabled = board(EVERY);
    change(&enabled, "a", "plan", &json!({}));
    assert_eq!(now(&enabled, &json!({}))["evals"]["refused"], json!(null));
}
