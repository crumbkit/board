//! Each flow in the configurable-flows design, and the flow the plugin
//! ships, reads as a `changes/flow/v1` and passes its committed schema.

use board_plugin::shape::schema_of;
use board_validate::Schema;
use schemars::generate::SchemaSettings;
use serde_json::json;

use super::*;

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

/// `value`, a flow, with each check's `controls` named `rules`, as Board
/// writes a flow stored before the rename.
fn renamed(mut value: serde_json::Value) -> serde_json::Value {
    for phase in value["phases"].as_array_mut().unwrap() {
        for check in phase["checks"].as_array_mut().unwrap() {
            let check = check.as_object_mut().unwrap();
            if let Some(rules) = check.remove("controls") {
                check.insert("rules".to_owned(), rules);
            }
        }
    }
    value
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
        assert_eq!(
            serde_json::to_value(&flow).unwrap(),
            renamed(value),
            "{name}"
        );
    }
}

/// The version of the shipped development flow from before the rename,
/// which changes started then follow. It names its rules `controls`, under
/// `/controls/`; it reads with them as `rules` and keeps its version path,
/// and the shipped flow names `/rules/` and has a new version.
#[test]
fn a_flow_stored_before_the_rename_reads_its_controls_as_rules_and_keeps_its_version_path() {
    let text = include_str!("flows-md/development-a9df0910bd037be9.json");
    let value: serde_json::Value = serde_json::from_str(text).unwrap();
    schema().validate(&value).unwrap();
    let flow: Flow = serde_json::from_str(text).unwrap();
    assert_eq!(serde_json::to_value(&flow).unwrap(), renamed(value.clone()));
    assert_eq!(problems(&flow, false), []);
    let stored = read::version_of("development", &value);
    assert_eq!(stored, "/flows/development/a9df0910bd037be9");
    assert_ne!(version_path(&development()), stored);
    let check = (flow.phases.iter().flat_map(|phase| &phase.checks))
        .find(|each| each.id == "lifecycle-declared")
        .unwrap();
    assert_eq!(
        check.rules.as_deref().unwrap(),
        ["/controls/{repo}/lifecycle-paths"]
    );
    let shipped = development();
    let check = (shipped.phases.iter().flat_map(|phase| &phase.checks))
        .find(|each| each.id == "lifecycle-declared")
        .unwrap();
    assert_eq!(
        check.rules.as_deref().unwrap(),
        ["/rules/{repo}/lifecycle-paths"]
    );
}

/// A check names its rules under `rules` or under `controls`, the name a
/// flow stored before the rename gives them, and the schema refuses both
/// at once, or neither.
#[test]
fn a_check_names_its_rules_under_one_name_and_the_schema_refuses_both_or_neither() {
    let schema = schema();
    let value: serde_json::Value =
        serde_json::from_str(include_str!("../../flows/small.json")).unwrap();
    let check = || value["phases"][1]["checks"][1].as_object().unwrap().clone();
    let rules = check()["rules"].clone();
    for (field, both, admitted) in [
        (Some("rules"), false, true),
        (Some("controls"), false, true),
        (Some("rules"), true, false),
        (None, false, false),
    ] {
        let mut shaped = check();
        shaped.remove("rules");
        if let Some(field) = field {
            shaped.insert(field.to_owned(), rules.clone());
        }
        if both {
            shaped.insert("controls".to_owned(), rules.clone());
        }
        let mut flow = value.clone();
        flow["phases"][1]["checks"][1] = serde_json::Value::Object(shaped);
        let said = format!("{field:?} both {both}");
        assert_eq!(schema.validate(&flow).is_ok(), admitted, "{said}");
        // serde refuses both names at once; a check with neither reads, and
        // flow-valid names the missing parameter.
        let read = serde_json::from_value::<Flow>(flow);
        assert_eq!(read.is_ok(), !both, "{said}");
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
    assert_eq!(
        plan.sections.as_deref().unwrap(),
        [
            "Problem",
            "Outcome",
            "Out of scope",
            "UX",
            "Data",
            "Failure paths",
            "Consumers",
            "Lifecycle"
        ]
    );
}

/// The stored version an open change follows, from before the development
/// flow gained Failure paths, Consumers and Lifecycle. The edit gives the
/// flow a new version and leaves this one reading as it did.
#[test]
fn a_stored_flow_version_still_reads_passes_its_schema_and_keeps_its_version_path() {
    let text = include_str!("flows-md/development-2bad23722b29b5a8.json");
    let value: serde_json::Value = serde_json::from_str(text).unwrap();
    schema().validate(&value).unwrap();
    let flow: Flow = serde_json::from_str(text).unwrap();
    assert_eq!(serde_json::to_value(&flow).unwrap(), renamed(value.clone()));
    assert_eq!(problems(&flow, false), []);
    let stored = read::version_of("development", &value);
    assert_eq!(stored, "/flows/development/2bad23722b29b5a8");
    assert_ne!(version_path(&development()), stored);
}

/// The `when` of the check `id` of `flow`.
fn named<'flow>(flow: &'flow Flow, id: &str) -> &'flow When {
    let mut checks = flow.phases.iter().flat_map(|phase| &phase.checks);
    let check = checks.find(|each| each.id == id).unwrap();
    check.when.as_ref().unwrap()
}

/// The stored version a change started before plans gave typed answers
/// follows: its checks read the plan's prose. Naming `answers` in the
/// shipped flow gives that flow a new version and leaves this one as it was.
#[test]
fn the_stored_prose_flow_keeps_its_version_path_and_the_shipped_flow_names_answers() {
    let text = include_str!("flows-md/development-60bd988ebac76d79.json");
    let value: serde_json::Value = serde_json::from_str(text).unwrap();
    schema().validate(&value).unwrap();
    let flow: Flow = serde_json::from_str(text).unwrap();
    assert_eq!(serde_json::to_value(&flow).unwrap(), renamed(value.clone()));
    assert_eq!(problems(&flow, false), []);
    let stored = read::version_of("development", &value);
    assert_eq!(stored, "/flows/development/60bd988ebac76d79");
    assert_ne!(version_path(&development()), stored);
    assert!(named(&flow, "data-signed").section_not.is_some());
    assert_eq!(named(&flow, "data-signed").answers, None);
    let shipped = development();
    let when = named(&shipped, "data-signed");
    assert_eq!(
        serde_json::to_value(when).unwrap(),
        json!({"answers": {"data": "stored"}})
    );
    assert_eq!(problems(&shipped, false), []);
}

/// A `when` takes each answer's two words, and the schema and the reader
/// both refuse another word.
#[test]
fn a_when_names_answers_by_their_words_and_refuses_another_word() {
    let with = |answers: serde_json::Value| {
        json!({
            "$type": "changes/flow/v1", "name": "one", "summary": "One phase.",
            "stop": {"until": "build"},
            "phases": [{"name": "build", "agent": "lead", "instruction": "Build.",
                        "checks": [{"id": "x", "kind": "tasks", "when": {"answers": answers}}]}]
        })
    };
    let good = with(json!({"ux": "mockups", "data": "none", "lifecycle": "record"}));
    schema().validate(&good).unwrap();
    let flow: Flow = serde_json::from_value(good.clone()).unwrap();
    assert_eq!(serde_json::to_value(&flow).unwrap(), good);
    let bad = with(json!({"data": "None:"}));
    assert!(schema().validate(&bad).is_err());
    assert!(serde_json::from_value::<Flow>(bad).is_err());
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
fn a_when_absent_path_with_an_unknown_placeholder_is_a_problem_and_a_known_one_is_not() {
    let check = |path: &str| json!({"id": "x", "kind": "tasks", "when": {"absent": {"path": path, "type": "a/b/v1"}}});
    assert_eq!(
        said(&one(&check("/changes/{who}"), "build", "Build."), false),
        [
            "phase build, check x: it names the unknown placeholder {who} (changes::flow::unknown_placeholder)"
        ]
    );
    assert_eq!(
        said(&one(&check("/changes/{name}"), "build", "Build."), false),
        Vec::<String>::new()
    );
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
        [
            "the stop names the phase review, which the flow lacks; name one of build or done (changes::flow::stop_not_a_phase)"
        ]
    );
    assert_eq!(
        said(&one(&fine, "build", "Build {branch}."), false),
        [
            "phase build, the instruction names the unknown placeholder {branch} (changes::flow::unknown_placeholder)"
        ]
    );
    assert_eq!(
        said(
            &one(&json!({"id": "x", "kind": "scripted"}), "build", "Build."),
            false
        ),
        [
            "phase build, check x: the kind scripted is not one this Board knows (changes::flow::unknown_kind)"
        ]
    );
    assert_eq!(
        said(
            &one(&json!({"id": "x", "kind": "exists"}), "build", "Build."),
            false
        ),
        [
            "phase build, check x: a check of kind exists needs path (changes::flow::missing_parameter)"
        ]
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
        [
            "phase build, check x: it names the unknown placeholder {who} (changes::flow::unknown_placeholder)"
        ]
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
          that tells an unattended agent how to ask (changes::flow::no_remedy)"
        ]
    );
    let mut twice = one(&fine, "build", "Build.");
    twice.phases[0]
        .checks
        .push(serde_json::from_value(fine).unwrap());
    assert_eq!(
        said(&twice, false),
        [
            "phase build, check x: another check of the flow has this id (changes::flow::duplicate_check)"
        ]
    );
}

/// The codes of the problems of a flow with `phases`, each with one check
/// of kind tasks and `when`, stopping at done.
fn codes_of(phases: &[&str], when: &serde_json::Value) -> Vec<&'static str> {
    let phases: Vec<serde_json::Value> = (phases.iter().enumerate())
        .map(|(at, name)| {
            json!({"name": name, "agent": "lead", "instruction": "Do it.",
                   "checks": [{"id": format!("c{at}"), "kind": "tasks", "when": when}]})
        })
        .collect();
    let flow: Flow = serde_json::from_value(json!({
        "$type": "changes/flow/v1", "name": "f", "summary": "A flow.",
        "stop": {"until": "done"}, "phases": phases
    }))
    .unwrap();
    problems(&flow, false)
        .iter()
        .map(|problem| problem.code)
        .collect()
}

#[test]
fn a_flow_with_no_phases_is_refused_and_one_with_a_phase_is_admitted() {
    assert_eq!(codes_of(&[], &json!({})), ["changes::flow::no_phases"]);
    assert_eq!(codes_of(&["build"], &json!({})), Vec::<&str>::new());
}

#[test]
fn a_flow_naming_two_phases_alike_is_refused_and_one_naming_them_apart_is_admitted() {
    assert_eq!(
        codes_of(&["build", "build"], &json!({})),
        ["changes::flow::duplicate_phase"]
    );
    assert_eq!(
        codes_of(&["build", "review"], &json!({})),
        Vec::<&str>::new()
    );
}

#[test]
fn a_phase_named_done_dropped_or_paused_is_refused_and_one_named_otherwise_is_admitted() {
    for reserved in ["done", "dropped", "paused"] {
        assert_eq!(
            codes_of(&["build", reserved], &json!({})),
            ["changes::flow::reserved_phase"],
            "{reserved}"
        );
    }
    assert_eq!(codes_of(&["build", "land"], &json!({})), Vec::<&str>::new());
}

#[test]
fn a_when_pattern_that_is_not_a_regular_expression_is_refused_and_one_that_is_is_admitted() {
    for key in ["section", "section_not"] {
        assert_eq!(
            codes_of(&["build"], &json!({key: {"Data": "^none("}})),
            ["changes::flow::bad_pattern"],
            "{key}"
        );
        assert_eq!(
            codes_of(&["build"], &json!({key: {"Data": "^none\\b"}})),
            Vec::<&str>::new(),
            "{key}"
        );
    }
}

/// A flow document with one check, `check`, in its one phase.
fn with_check(check: &serde_json::Value) -> serde_json::Value {
    json!({
        "$type": "changes/flow/v1", "name": "one", "summary": "One phase.",
        "stop": {"until": "build"},
        "phases": [{"name": "build", "agent": "lead", "instruction": "Build.",
                    "checks": [check]}]
    })
}

/// The code the schema refuses `check` with.
fn refused(check: &serde_json::Value) -> board_model::Code {
    schema().validate(&with_check(check)).unwrap_err().code()
}

#[test]
fn a_check_holding_another_kinds_parameter_is_refused_and_its_twin_without_it_is_admitted() {
    let exists = json!({"id": "x", "kind": "exists", "path": "/flows/{draft}"});
    let mut stray = exists.clone();
    stray["scope"] = json!("full");
    assert_eq!(refused(&stray), board_model::Code::RecordInvalid);
    schema().validate(&with_check(&exists)).unwrap();
}

#[test]
fn a_check_without_its_kinds_required_parameter_is_refused_and_its_twin_with_it_is_admitted() {
    let exists = json!({"id": "x", "kind": "exists"});
    assert_eq!(refused(&exists), board_model::Code::RecordInvalid);
    let mut named = exists.clone();
    named["path"] = json!("/flows/{draft}");
    schema().validate(&with_check(&named)).unwrap();
}

/// `min` belongs to two kinds, so each of them admits it and every other
/// kind refuses it.
#[test]
fn a_tasks_check_with_min_is_refused_and_field_and_records_checks_with_min_are_admitted() {
    assert_eq!(
        refused(&json!({"id": "x", "kind": "tasks", "min": 2})),
        board_model::Code::RecordInvalid
    );
    schema()
        .validate(&with_check(&json!({"id": "x", "kind": "tasks"})))
        .unwrap();
    schema()
        .validate(&with_check(
            &json!({"id": "x", "kind": "field", "field": "pulls", "min": 2}),
        ))
        .unwrap();
    schema()
        .validate(&with_check(
            &json!({"id": "x", "kind": "records", "type": "changes/plan/v1", "min": 2}),
        ))
        .unwrap();
}

#[test]
fn a_check_holding_a_field_no_kind_names_is_refused_and_its_twin_without_it_is_admitted() {
    assert_eq!(
        refused(&json!({"id": "x", "kind": "tasks", "scripted": true})),
        board_model::Code::RecordInvalid
    );
    schema()
        .validate(&with_check(&json!({"id": "x", "kind": "tasks"})))
        .unwrap();
}

/// The codes of the problems of a flow called `name` with one phase.
fn codes_named(name: &str) -> Vec<&'static str> {
    let flow: Flow = serde_json::from_value(json!({
        "$type": "changes/flow/v1", "name": name, "summary": "A flow.",
        "stop": {"until": "done"},
        "phases": [{"name": "build", "agent": "lead", "instruction": "Do it.", "checks": []}]
    }))
    .unwrap();
    problems(&flow, false)
        .iter()
        .map(|problem| problem.code)
        .collect()
}

#[test]
fn a_flow_named_repos_is_refused_and_one_named_repo_is_admitted() {
    assert_eq!(codes_named("repos"), ["changes::flow::reserved_name"]);
    assert_eq!(codes_named("repo"), Vec::<&str>::new());
}

mod version;
