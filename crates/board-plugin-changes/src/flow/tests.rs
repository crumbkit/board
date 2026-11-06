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
