//! Each flow in the configurable-flows design, and the flow the plugin
//! ships, reads as a `changes/flow/v1` and passes its committed schema.

use board_plugin::shape::schema_of;
use board_validate::Schema;
use schemars::generate::SchemaSettings;
use serde_json::json;

use super::*;
use crate::gates::SECTIONS;

/// The four flows of the design's flows.md, copied verbatim.
const DESIGNED: [(&str, &str); 4] = [
    ("development", include_str!("flows-md/development.json")),
    ("small", include_str!("flows-md/small.json")),
    (
        "flow-authoring",
        include_str!("flows-md/flow-authoring.json"),
    ),
    ("team", include_str!("flows-md/team.json")),
];

fn schema() -> Schema {
    Schema::compile(schema_of::<Flow>(SchemaSettings::draft2020_12())).unwrap()
}

#[test]
fn each_designed_flow_reads_as_a_flow_and_passes_its_schema() {
    let schema = schema();
    for (name, text) in DESIGNED {
        let flow: Flow =
            serde_json::from_str(text).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(flow.name, name);
        let value: serde_json::Value = serde_json::from_str(text).unwrap();
        schema
            .validate(&value)
            .unwrap_or_else(|error| panic!("{name}: {error:?}"));
        assert_eq!(serde_json::to_value(&flow).unwrap(), value, "{name}");
    }
}

#[test]
fn the_shipped_development_flow_passes_its_schema_and_orders_plan_build_review_land() {
    let value: serde_json::Value =
        serde_json::from_str(include_str!("../../flows/development.json")).unwrap();
    schema().validate(&value).unwrap();
    let flow = development();
    let names: Vec<&str> = flow
        .phases
        .iter()
        .map(|phase| phase.name.as_str())
        .collect();
    assert_eq!(names, ["plan", "build", "review", "land"]);
    let order: Vec<Option<&str>> = ["plan", "build", "review", "land", "done"]
        .iter()
        .map(|name| flow.after(name))
        .collect();
    assert_eq!(
        order,
        [
            Some("build"),
            Some("review"),
            Some("land"),
            Some("done"),
            None
        ]
    );
    let plan = &flow.phase("plan").unwrap().checks[0];
    assert_eq!(plan.sections.as_deref().unwrap(), SECTIONS);
}

/// A check of a kind this Board does not know still reads, so the flow
/// reads; the schema refuses it, and admits its twin of a known kind.
#[test]
fn a_check_of_an_unknown_kind_reads_and_the_schema_refuses_it() {
    let with = |kind: &str| {
        json!({
            "$type": "changes/flow/v1", "name": "one", "summary": "One phase.",
            "stop": {"until": "build"},
            "phases": [{"name": "build", "agent": "lead", "instruction": "Build.",
                        "checks": [{"id": "x", "kind": kind}]}]
        })
    };
    let flow: Flow = serde_json::from_value(with("scripted")).unwrap();
    assert_eq!(
        flow.phases[0].checks[0].kind,
        Open::Other("scripted".to_owned())
    );
    assert_eq!(
        schema().validate(&with("scripted")).unwrap_err().code(),
        board_model::Code::RecordInvalid
    );
    schema().validate(&with("tasks")).unwrap();
}

/// Each shipped flow can run, and the flow-authoring flow passes even the
/// check a drafted flow must: a remedy on every check the owner passes.
#[test]
fn each_shipped_flow_has_no_problem_and_the_flow_authoring_flow_passes_as_a_draft() {
    for (name, _) in SHIPPED {
        let flow = shipped(name).unwrap();
        assert_eq!(problems(&flow, false), [], "{name}");
        let value: serde_json::Value =
            serde_json::from_str(SHIPPED.iter().find(|each| each.0 == name).unwrap().1).unwrap();
        schema().validate(&value).unwrap();
    }
    assert_eq!(problems(&shipped("flow-authoring").unwrap(), true), []);
}

/// A flow with one check, `check`, in its one phase `build`, stopping at
/// `stop`.
fn one(check: &serde_json::Value, stop: &str, instruction: &str) -> Flow {
    serde_json::from_value(json!({
        "$type": "changes/flow/v1", "name": "one", "summary": "One phase.",
        "stop": {"until": stop},
        "phases": [{"name": "build", "agent": "lead", "instruction": instruction,
                    "checks": [check]}]
    }))
    .unwrap()
}

fn said(flow: &Flow, drafted: bool) -> Vec<String> {
    problems(flow, drafted)
        .iter()
        .map(ToString::to_string)
        .collect()
}

#[test]
fn a_flow_with_each_problem_says_where_and_its_twin_without_it_has_none() {
    let fine = json!({"id": "x", "kind": "tasks"});
    assert_eq!(
        said(&one(&fine, "build", "Build {change}."), true),
        Vec::<String>::new()
    );
    assert_eq!(
        said(&one(&fine, "done", "Build."), true),
        Vec::<String>::new()
    );
    assert_eq!(
        said(&one(&fine, "review", "Build."), false),
        ["the stop names the phase review, which the flow lacks; name one of build or done"]
    );
    assert_eq!(
        said(&one(&fine, "build", "Build {branch}."), false),
        ["phase build, the instruction names the unknown placeholder {branch}"]
    );
    assert_eq!(
        said(
            &one(&json!({"id": "x", "kind": "scripted"}), "build", "Build."),
            false
        ),
        ["phase build, check x: the kind scripted is not one this Board knows"]
    );
    assert_eq!(
        said(
            &one(&json!({"id": "x", "kind": "exists"}), "build", "Build."),
            false
        ),
        ["phase build, check x: a check of kind exists needs path"]
    );
    assert_eq!(
        said(
            &one(
                &json!({"id": "x", "kind": "exists", "path": "/flows/{draft}"}),
                "build",
                "B."
            ),
            false
        ),
        Vec::<String>::new()
    );
    assert_eq!(
        said(
            &one(
                &json!({"id": "x", "kind": "tasks", "remedy": "Ask {who}."}),
                "build",
                "Build."
            ),
            false
        ),
        ["phase build, check x: it names the unknown placeholder {who}"]
    );
    let asks = json!({"id": "x", "kind": "tasks", "actor": "owner"});
    assert_eq!(
        said(&one(&asks, "build", "Build."), false),
        Vec::<String>::new()
    );
    assert_eq!(
        said(&one(&asks, "build", "Build."), true),
        [
            "phase build, check x: only the owner or a reviewer can pass it, and it has no remedy \
          that tells an unattended agent how to ask"
        ]
    );
    let mut twice = one(&fine, "build", "Build.");
    twice.phases[0]
        .checks
        .push(serde_json::from_value(fine).unwrap());
    assert_eq!(
        said(&twice, false),
        ["phase build, check x: another check of the flow has this id"]
    );
}
