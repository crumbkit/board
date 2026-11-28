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

/// `value`, a flow, with each check's `controls` named `rules` and each
/// former check kind under its new name, as Board writes a flow stored
/// before the rename.
fn renamed(mut value: serde_json::Value) -> serde_json::Value {
    for phase in value["phases"].as_array_mut().unwrap() {
        for check in phase["checks"].as_array_mut().unwrap() {
            let check = check.as_object_mut().unwrap();
            if let Some(rules) = check.remove("controls") {
                check.insert("rules".to_owned(), rules);
            }
            for (former, kind) in FORMER_KINDS {
                if check.get("kind") == Some(&json!(former)) {
                    check.insert("kind".to_owned(), json!(kind.as_str()));
                }
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
    let stored = board_model::flow_version("development", &value);
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
    let stored = board_model::flow_version("development", &value);
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
    let stored = board_model::flow_version("development", &value);
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

/// The stored version an open change follows, from before the flow checked
/// a build against its mockups. The shipped flow has a new version.
#[test]
fn the_stored_flow_without_design_checks_keeps_its_version_path_and_the_shipped_flow_has_them() {
    let text = include_str!("flows-md/development-da44e719e601120e.json");
    let value: serde_json::Value = serde_json::from_str(text).unwrap();
    schema().validate(&value).unwrap();
    let flow: Flow = serde_json::from_str(text).unwrap();
    assert_eq!(problems(&flow, false), []);
    let stored = board_model::flow_version("development", &value);
    assert_eq!(stored, "/flows/development/da44e719e601120e");
    assert_ne!(version_path(&development()), stored);
    let has = |flow: &Flow, id: &str| {
        (flow.phases.iter().flat_map(|phase| &phase.checks)).any(|each| each.id == id)
    };
    assert!(!has(&flow, "design-conformance") && !has(&flow, "mockup-versions"));
    let shipped = development();
    assert!(has(&shipped, "design-conformance") && has(&shipped, "mockup-versions"));
    assert_eq!(problems(&shipped, false), []);
}

/// The stored version changes started before decisions 1306Z, 1308Z and
/// 1318Z follow: rechecks block build, every plan needs approval and a
/// review covers only the head. The shipped flow has a new version.
#[test]
fn the_stored_flow_before_the_retro_keeps_its_version_path_and_the_shipped_flow_moves_rechecks() {
    let text = include_str!("flows-md/development-3029fae590f5a628.json");
    let value: serde_json::Value = serde_json::from_str(text).unwrap();
    schema().validate(&value).unwrap();
    let flow: Flow = serde_json::from_str(text).unwrap();
    assert_eq!(problems(&flow, false), []);
    let stored = board_model::flow_version("development", &value);
    assert_eq!(stored, "/flows/development/3029fae590f5a628");
    assert_ne!(version_path(&development()), stored);
    let phase_of = |flow: &Flow, id: &str| {
        (flow.phases.iter())
            .find(|phase| phase.checks.iter().any(|each| each.id == id))
            .map(|phase| phase.name.clone())
    };
    assert_eq!(phase_of(&flow, "rechecks").as_deref(), Some("build"));
    let shipped = development();
    assert_eq!(phase_of(&shipped, "rechecks").as_deref(), Some("land"));
    let rechecks = (shipped.phases.iter().flat_map(|phase| &phase.checks))
        .find(|each| each.id == "rechecks")
        .unwrap();
    assert_eq!(rechecks.blocks, Some(false));
    assert_eq!(
        serde_json::to_value(named(&shipped, "owner-approval")).unwrap(),
        json!({"answers_not": {"ux": "none", "data": "none"}})
    );
}

/// An `answers_not` takes each answer's two words; the schema and the
/// reader both refuse an answer that does not exist and a word that is
/// not one of its two.
#[test]
fn an_answers_not_naming_an_unknown_answer_or_word_is_refused_and_its_twin_is_admitted() {
    let with = |answers: serde_json::Value| {
        json!({
            "$type": "changes/flow/v1", "name": "one", "summary": "One phase.",
            "stop": {"until": "build"},
            "phases": [{"name": "build", "agent": "lead", "instruction": "Build.",
                        "checks": [{"id": "x", "kind": "tasks", "when": {"answers_not": answers}}]}]
        })
    };
    let good = with(json!({"ux": "none", "data": "none"}));
    schema().validate(&good).unwrap();
    let flow: Flow = serde_json::from_value(good.clone()).unwrap();
    assert_eq!(serde_json::to_value(&flow).unwrap(), good);
    for bad in [json!({"ui": "none"}), json!({"data": "nothing"})] {
        let bad = with(bad);
        assert!(schema().validate(&bad).is_err(), "{bad}");
        assert!(serde_json::from_value::<Flow>(bad).is_err());
    }
}

/// A check's `unchanged_by` names its rules with known placeholders only.
#[test]
fn an_unchanged_by_rule_with_an_unknown_placeholder_is_a_problem_and_a_known_one_is_not() {
    let check = |rule: &str| {
        json!({"id": "x", "kind": "records", "type": "review/report/v1",
               "where": {"at_head": "range", "unchanged_by": [rule]}})
    };
    assert_eq!(
        said(
            &one(&check("/rules/{who}/record-paths"), "build", "B."),
            false
        ),
        [
            "phase build, check x: it names the unknown placeholder {who} (changes::flow::unknown_placeholder)"
        ]
    );
    assert_eq!(
        said(
            &one(&check("/rules/{repo}/record-paths"), "build", "B."),
            false
        ),
        Vec::<String>::new()
    );
}

/// Decision 1308Z in the small flow: a diff that edits a lifecycle path
/// passes once the change has its lifecycle record, and is refused
/// without one; a UI or data path still needs the development flow.
#[test]
fn the_small_flow_passes_a_lifecycle_diff_with_a_record_and_refuses_one_without() {
    use crate::change::Referenced;
    use crate::gates::{Observed, Status, gate};
    let rule = "/rules/x/lifecycle-paths";
    let small = |phase: &str, recorded: bool| {
        let change = crate::change::from_value(json!({
            "$type": "changes/change/v1", "title": "T", "mount": "/repos/x",
            "phase": phase, "head": "a3f9c0de1234567890abcdef1234567890abcdef",
        }))
        .unwrap();
        let mut observed = Observed::new(
            "/changes/t",
            change,
            crate::records::Kind::Build,
            Referenced::default(),
        );
        observed.flow = shipped("small");
        observed.controlled.insert(
            rule.to_owned(),
            Some(vec!["/repos/x/src/sync.rs".to_owned()]),
        );
        if recorded {
            observed
                .records_held
                .insert("/changes/t/lifecycle".to_owned());
        }
        gate(&observed)
    };
    for (phase, id) in [
        ("build", "lifecycle-recorded"),
        ("review", "lifecycle-paths"),
        ("land", "lifecycle-paths-at-land"),
    ] {
        let without = small(phase, false);
        let check = without.iter().find(|each| each.check == id).unwrap();
        assert_eq!(check.status, Status::Missing, "{phase}");
        assert!(
            check
                .remedy
                .as_deref()
                .unwrap()
                .contains("board changes lifecycle")
        );
        let with = small(phase, true);
        assert!(!with.iter().any(|each| each.check == id), "{phase}");
    }
    let built = small("build", true);
    let not_small = built.iter().find(|each| each.check == "not-small").unwrap();
    assert_eq!(not_small.status, Status::Pass);
}

/// Each built-in plugin's name and its capabilities' names, read from the
/// `board plugin list --json` golden that board-cli's help test checks
/// against the built binary.
fn built_in_capabilities() -> Vec<(String, Vec<String>)> {
    let listed: serde_json::Value = serde_json::from_str(include_str!(
        "../../../board-cli/src/tests/help/examples/board-plugin-list-json.txt"
    ))
    .unwrap();
    let names = |values: &serde_json::Value| -> Vec<String> {
        (values.as_array().unwrap().iter())
            .map(|each| each["name"].as_str().unwrap().to_owned())
            .collect()
    };
    (listed["data"].as_array().unwrap().iter())
        .map(|plugin| {
            let name = plugin["name"].as_str().unwrap().to_owned();
            (name, names(&plugin["capabilities"]))
        })
        .collect()
}

/// Board's own top-level words, read from the `GOALS` table that orders
/// `board -h`, whose test fails when a root command is missing from it.
fn board_words() -> Vec<String> {
    let render = include_str!("../../../board-cli/src/render.rs");
    let start = render.find("pub(crate) const GOALS").unwrap();
    let goals = &render[start..];
    let goals = &goals[..goals.find("\n);").unwrap()];
    let lists = regex::Regex::new(r"&\[([^\]]*)\]").unwrap();
    let word = regex::Regex::new(r#""([a-z-]+)""#).unwrap();
    (lists.captures_iter(goals))
        .flat_map(|list| {
            (word.captures_iter(&list[1]))
                .map(|each| each[1].to_owned())
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Each Board word that has subcommands and the names of those subcommands,
/// read from the `board commands list` golden that board-cli's help test
/// checks against the built binary. A word the golden lists with no
/// subcommand rows, such as `put`, is left out. The golden does not list
/// every root word: `skills`, `completion`, `commands` and `schemas` have
/// subcommands it leaves out, so their second word goes unchecked.
fn board_subcommands() -> Vec<(String, Vec<String>)> {
    let listed = include_str!("../../../board-cli/src/tests/help/examples/board-commands-list.txt");
    let mut lines = listed.lines();
    let width = lines.next().unwrap().find("ABOUT").unwrap();
    let mut words: Vec<(String, Vec<String>)> = Vec::new();
    for line in lines {
        let path: Vec<&str> = line[..width].split_whitespace().collect();
        let [_, word, subcommand] = path[..] else {
            continue;
        };
        match words.iter_mut().find(|(name, _)| name == word) {
            Some((_, subcommands)) => subcommands.push(subcommand.to_owned()),
            None => words.push((word.to_owned(), vec![subcommand.to_owned()])),
        }
    }
    words
}

/// Each command in backticks in `text` that no built-in plugin offers or
/// Board does not have. A command is a span that starts with `board`, or a
/// span that starts with a built-in plugin's dotted capability, such as
/// `changes.attach --task`. A Board word with subcommands in
/// [`board_subcommands`] must be followed by one of them.
fn unknown_commands(
    text: &str,
    plugins: &[(String, Vec<String>)],
    words: &[String],
) -> Vec<String> {
    let spans =
        regex::Regex::new(r"`(board(?: [^`]*)?|[a-z][a-z-]*\.[a-z][a-z-]*(?: [^`]*)?)`").unwrap();
    let subcommands = board_subcommands();
    let offers = |plugin: &str, capability: &str| {
        (plugins.iter())
            .any(|(name, offered)| name == plugin && offered.iter().any(|each| each == capability))
    };
    let known = |command: &str| {
        let head = command.split_whitespace().next().unwrap_or_default();
        if let Some((plugin, capability)) = head.split_once('.') {
            return !plugins.iter().any(|(name, _)| name == plugin) || offers(plugin, capability);
        }
        let mut tokens = command.split_whitespace().skip(1).peekable();
        while let Some(flag) = tokens.next_if(|token| token.starts_with('-')) {
            if flag == "-C" {
                tokens.next();
            }
        }
        let Some(first) = tokens.next() else {
            return true;
        };
        if first == "call" {
            let target = tokens.next().unwrap_or_default();
            return (target.split_once('.'))
                .is_some_and(|(plugin, capability)| offers(plugin, capability));
        }
        if words.iter().any(|word| word == first) {
            let Some((_, offered)) = subcommands.iter().find(|(word, _)| word == first) else {
                return true;
            };
            let mut rest = tokens.skip_while(|token| token.starts_with('-'));
            return rest
                .next()
                .is_none_or(|second| offered.iter().any(|each| each == second));
        }
        match tokens.next() {
            Some(capability) => offers(first, capability),
            None => plugins.iter().any(|(name, _)| name == first),
        }
    };
    (spans.captures_iter(text))
        .map(|span| span[1].to_owned())
        .filter(|command| !known(command))
        .collect()
}

/// A remedy naming `board runs run`, which no plugin offers, is caught, and
/// its twin naming `board changes attach` and Board's own `put` is not.
#[test]
fn a_command_no_plugin_offers_is_caught_and_its_twin_that_exists_is_not() {
    let (plugins, words) = (built_in_capabilities(), board_words());
    assert!(plugins.iter().any(|(name, _)| name == "runs"));
    assert!(words.iter().any(|word| word == "put"));
    let caught = unknown_commands(
        "Run {suite} with `board runs run` and attach it.",
        &plugins,
        &words,
    );
    assert_eq!(caught, ["board runs run"]);
    let twin = "Write it with `board put --new --var suite=<name>`, attach it with `board changes \
        attach --path {change} --ref <run>`, or call `board --personal call changes.next`.";
    assert_eq!(
        unknown_commands(twin, &plugins, &words),
        Vec::<String>::new()
    );
}

/// A remedy naming the dotted capability `runs.run`, which the runs plugin
/// does not offer, is caught, and its twin naming `changes.attach --task`,
/// which exists, is not. A dotted span whose first word names no built-in
/// plugin, such as a file name, is not a command.
#[test]
fn a_dotted_capability_no_plugin_offers_is_caught_and_its_twin_that_exists_is_not() {
    let (plugins, words) = (built_in_capabilities(), board_words());
    let caught = unknown_commands(
        "Run {suite} with `runs.run` and attach it.",
        &plugins,
        &words,
    );
    assert_eq!(caught, ["runs.run"]);
    let twin = "Attach it with `changes.attach --task <path>` and read `home.toml`.";
    assert_eq!(
        unknown_commands(twin, &plugins, &words),
        Vec::<String>::new()
    );
}

/// A Board word that has subcommands, such as `plugin` or `mount`, followed
/// by a word that is not one of them is caught, and its twin naming
/// `board plugin list` and `board mount add` is not. A word with no
/// subcommands, such as `put`, takes a path as its second word.
#[test]
fn a_board_word_with_a_subcommand_that_does_not_exist_is_caught() {
    let (plugins, words) = (built_in_capabilities(), board_words());
    let caught = unknown_commands(
        "Try `board plugin bogus` or `board -C ~/Code/board mount bogus /x`.",
        &plugins,
        &words,
    );
    assert_eq!(
        caught,
        ["board plugin bogus", "board -C ~/Code/board mount bogus /x"]
    );
    let twin = "Read `board plugin list --json`, `board mount add <clone> /repos/x` and \
        `board put /suites/<name>`.";
    assert_eq!(
        unknown_commands(twin, &plugins, &words),
        Vec::<String>::new()
    );
}

/// Every command in backticks that starts with `board`, in every shipped
/// flow's instructions and in the remedy each check prints, its flow's own
/// or its kind's, names a capability a built-in plugin offers or one of
/// Board's own words.
#[test]
fn every_board_command_in_a_shipped_flow_names_a_command_that_exists() {
    let (plugins, words) = (built_in_capabilities(), board_words());
    let values = crate::gates::remedy::PLACEHOLDERS.map(|name| (name, None));
    let mut unknown = Vec::new();
    for (name, _) in SHIPPED {
        let flow = shipped(name).unwrap();
        for phase in &flow.phases {
            for command in unknown_commands(&phase.instruction, &plugins, &words) {
                unknown.push(format!("{name} {} instruction: {command}", phase.name));
            }
            for check in &phase.checks {
                let remedy = crate::gates::remedy::of(check, &values);
                for command in unknown_commands(&remedy, &plugins, &words) {
                    unknown.push(format!("{name} {} {}: {command}", phase.name, check.id));
                }
            }
        }
    }
    assert_eq!(unknown, Vec::<String>::new());
}
