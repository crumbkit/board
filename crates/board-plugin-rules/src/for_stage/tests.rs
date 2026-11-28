//! `for-stage` over a recording host: which registrations a slot receives,
//! how the word limit leaves some out, how a missing file refuses one row,
//! and that `check` and `for-path` never list a stage rule.

use serde_json::{Value, json};

use super::*;
use crate::check::check;
use crate::check::tests::{Recording, enabling};
use crate::for_path::for_path;

/// A rule registered against `stage` and `agent` that provides `provide`
/// from `file`, with `fields` added.
fn registration(stage: &str, agent: &str, provide: &str, file: &str, fields: &Value) -> String {
    let mut record = json!({
        "$type": "rules/rule/v1",
        "requirement": "An agent at this stage uses what the rule provides.",
        "purpose": "Every repository works the same way.",
        "show": {"stage": stage, "agent": agent},
        "provides": {"provide": provide, "file": file},
    });
    for (name, value) in fields.as_object().unwrap() {
        record[name] = value.clone();
    }
    record.to_string()
}

/// A board holding `files`, with the rules plugin enabled.
fn board(files: &[(&str, &str)]) -> (board_scenario::Scenario, Recording) {
    enabling(&["rules"], files)
}

fn asked(stage: &str, agent: &str) -> Value {
    json!({"path": "/", "stage": stage, "agent": agent})
}

#[test]
fn a_slot_receives_only_its_own_registrations_in_path_order() {
    let summary = |text: &str| json!({"summary": text});
    let (_scenario, host) = board(&[
        ("skills/tdd/SKILL.md", "# TDD\n"),
        ("skills/review/SKILL.md", "# Review\n"),
        ("context/orient.md", "Read the plan first.\n"),
        (
            "rules/imposed/b-tdd.json",
            &registration(
                "orient",
                "builder",
                "skill",
                "/skills/tdd/SKILL.md",
                &summary("Write each test first."),
            ),
        ),
        (
            "rules/imposed/a-orient.json",
            &registration(
                "orient",
                "builder",
                "context",
                "/context/orient.md",
                &json!({}),
            ),
        ),
        (
            "rules/imposed/c-other-agent.json",
            &registration(
                "orient",
                "reviewer",
                "skill",
                "/skills/review/SKILL.md",
                &json!({}),
            ),
        ),
        (
            "rules/imposed/d-other-stage.json",
            &registration(
                "review",
                "builder",
                "skill",
                "/skills/review/SKILL.md",
                &json!({}),
            ),
        ),
        (
            "rules/outside.json",
            &registration(
                "orient",
                "builder",
                "skill",
                "/skills/review/SKILL.md",
                &json!({}),
            ),
        ),
    ]);
    let output = for_stage(&host, &asked("orient", "builder")).unwrap();
    assert_eq!(
        output,
        json!({
            "stage": "orient",
            "agent": "builder",
            "skills": [{"rule": "/rules/imposed/b-tdd", "name": "tdd", "file": "/skills/tdd/SKILL.md", "summary": "Write each test first."}],
            "context": [{"rule": "/rules/imposed/a-orient", "text": "Read the plan first."}],
            "left_out": [],
            "refused": [],
        })
    );
    let output = for_stage(&host, &asked("orient", "reviewer")).unwrap();
    assert_eq!(
        output["skills"],
        json!([{
            "rule": "/rules/imposed/c-other-agent",
            "name": "review",
            "file": "/skills/review/SKILL.md",
            "summary": "An agent at this stage uses what the rule provides.",
        }])
    );
}

#[test]
fn registrations_past_150_words_are_left_out_by_path_and_a_later_one_that_fits_is_kept() {
    let long = vec!["word"; 100].join(" ");
    let short = "Read the plan.";
    let (_scenario, host) = board(&[
        ("context/first.md", &long),
        ("context/second.md", &long),
        ("context/third.md", short),
        (
            "rules/imposed/1.json",
            &registration("plan", "lead", "context", "/context/first.md", &json!({})),
        ),
        (
            "rules/imposed/2.json",
            &registration("plan", "lead", "context", "/context/second.md", &json!({})),
        ),
        (
            "rules/imposed/3.json",
            &registration("plan", "lead", "context", "/context/third.md", &json!({})),
        ),
    ]);
    let output = for_stage(&host, &asked("plan", "lead")).unwrap();
    let kept: Vec<&Value> = (output["context"].as_array().unwrap().iter())
        .map(|each| &each["rule"])
        .collect();
    assert_eq!(
        kept,
        [&json!("/rules/imposed/1"), &json!("/rules/imposed/3")]
    );
    assert_eq!(output["left_out"], json!(["/rules/imposed/2"]));
    assert!(words(&output["skills"]) + words(&output["context"]) <= STAGE_WORDS);
}

/// The words of every string in `value`.
fn words(value: &Value) -> usize {
    match value {
        Value::String(text) => text.split_whitespace().count(),
        Value::Array(items) => items.iter().map(words).sum(),
        Value::Object(fields) => fields.values().map(words).sum(),
        _ => 0,
    }
}

#[test]
fn a_registration_whose_file_is_missing_is_one_refused_row_and_the_others_resolve() {
    let (_scenario, host) = board(&[
        ("skills/tdd/SKILL.md", "# TDD\n"),
        (
            "rules/imposed/gone.json",
            &registration(
                "build",
                "builder",
                "skill",
                "/skills/gone/SKILL.md",
                &json!({}),
            ),
        ),
        (
            "rules/imposed/tdd.json",
            &registration(
                "build",
                "builder",
                "skill",
                "/skills/tdd/SKILL.md",
                &json!({}),
            ),
        ),
    ]);
    let checked = check(&host, &json!({})).unwrap();
    assert_eq!(checked["passed"], json!(true), "{checked}");
    let output = for_stage(&host, &asked("build", "builder")).unwrap();
    let skills: Vec<&Value> = (output["skills"].as_array().unwrap().iter())
        .map(|each| &each["name"])
        .collect();
    assert_eq!(skills, [&json!("tdd")]);
    let refused = output["refused"].as_array().unwrap();
    assert_eq!(refused.len(), 1, "{output}");
    assert_eq!(refused[0]["rule"], json!("/rules/imposed/gone"));
    assert_eq!(refused[0]["code"], json!("rules::stage::file_missing"));
    assert_eq!(output["left_out"], json!([]));
}

#[test]
fn a_stage_rule_without_provides_is_invalid_so_check_fails_and_its_twin_passes() {
    let mut record: Value = serde_json::from_str(&registration(
        "land",
        "lead",
        "skill",
        "/skills/tdd/SKILL.md",
        &json!({}),
    ))
    .unwrap();
    let twin = record.to_string();
    record.as_object_mut().unwrap().remove("provides");
    let (_scenario, host) = board(&[
        ("skills/tdd/SKILL.md", "# TDD\n"),
        ("rules/imposed/empty.json", &record.to_string()),
    ]);
    let refused = check(&host, &json!({})).unwrap_err();
    assert_eq!(refused.code, "rules::rule::degraded");
    assert_eq!(
        refused.fields["rules"],
        json!([{"rule": "/rules/imposed/empty", "code": "board::record::invalid"}])
    );
    let (_scenario, host) = board(&[
        ("skills/tdd/SKILL.md", "# TDD\n"),
        ("rules/imposed/empty.json", &twin),
    ]);
    assert_eq!(check(&host, &json!({})).unwrap()["passed"], json!(true));
}

#[test]
fn a_stage_rule_outside_the_imposed_set_whose_file_is_missing_fails_check() {
    let (_scenario, host) = board(&[(
        "rules/elsewhere/gone.json",
        &registration(
            "build",
            "builder",
            "skill",
            "/skills/gone/SKILL.md",
            &json!({}),
        ),
    )]);
    let refused = check(&host, &json!({})).unwrap_err();
    assert_eq!(refused.code, "rules::rule::degraded");
    assert_eq!(
        refused.fields["rules"],
        json!([{"rule": "/rules/elsewhere/gone", "code": "board::ref::dangling"}])
    );
}

#[test]
fn an_imposed_stage_rule_whose_source_dangles_fails_check_and_is_refused_by_for_stage() {
    let source = json!({"sources": ["/evals/nothing-here"]});
    let (_scenario, host) = board(&[
        ("skills/tdd/SKILL.md", "# TDD\n"),
        (
            "rules/imposed/bad-source.json",
            &registration("build", "builder", "skill", "/skills/tdd/SKILL.md", &source),
        ),
    ]);
    let refused = check(&host, &json!({})).unwrap_err();
    assert_eq!(
        refused.fields["rules"],
        json!([{"rule": "/rules/imposed/bad-source", "code": "board::ref::dangling"}])
    );
    let output = for_stage(&host, &asked("build", "builder")).unwrap();
    assert_eq!(output["skills"], json!([]));
    assert_eq!(
        output["refused"],
        json!([{
            "rule": "/rules/imposed/bad-source",
            "code": "rules::stage::degraded",
            "message": "Board marks the rule /rules/imposed/bad-source degraded with board::ref::dangling; run `board check /rules/imposed/bad-source` for why",
        }])
    );
    let (_scenario, host) = board(&[
        ("skills/tdd/SKILL.md", "# TDD\n"),
        (
            "rules/imposed/bad-source.json",
            &registration(
                "build",
                "builder",
                "skill",
                "/skills/tdd/SKILL.md",
                &json!({}),
            ),
        ),
    ]);
    assert_eq!(check(&host, &json!({})).unwrap()["passed"], json!(true));
    let output = for_stage(&host, &asked("build", "builder")).unwrap();
    assert_eq!(output["skills"].as_array().unwrap().len(), 1);
}

#[test]
fn the_output_schema_types_paths_as_board_paths_and_context_text_as_prose() {
    let manifest = crate::manifest();
    let capability = (manifest["capabilities"].as_array().unwrap().iter())
        .find(|capability| capability["name"] == "for-stage")
        .unwrap();
    let output = &capability["outputSchema"];
    let defs = &output["$defs"];
    assert_eq!(defs["Skill"]["properties"]["file"]["format"], "board-path");
    assert_eq!(defs["Skill"]["properties"]["rule"]["format"], "board-path");
    assert_eq!(
        output["properties"]["left_out"]["items"]["format"],
        "board-path"
    );
    assert_eq!(defs["Context"]["properties"]["text"]["x-board-prose"], true);
    let description = capability["description"].as_str().unwrap();
    assert!(description.contains("does not yet narrow"), "{description}");
}

#[test]
fn an_unknown_stage_or_agent_kind_is_refused_and_a_listed_one_is_answered() {
    let (_scenario, host) = board(&[]);
    let refused = for_stage(&host, &asked("deploy", "builder")).unwrap_err();
    assert_eq!(refused.code, "rules::stage::unknown");
    assert_eq!(
        refused.message,
        "stage \"deploy\" is not one of orient, plan, mockup, build, review, land, handoff"
    );
    let refused = for_stage(&host, &asked("build", "designer")).unwrap_err();
    assert_eq!(refused.code, "rules::stage::unknown");
    assert_eq!(
        refused.message,
        "agent kind \"designer\" is not one of lead, ux, builder, reviewer"
    );
    let answered = for_stage(&host, &asked("build", "builder")).unwrap();
    assert_eq!(answered["skills"], json!([]));
}

#[test]
fn a_path_board_cannot_resolve_is_refused_and_one_it_resolves_is_answered() {
    let (_scenario, host) = board(&[("docs/a.md", "A.\n")]);
    let input = json!({"path": "/repos/missing", "stage": "orient", "agent": "lead"});
    let refused = for_stage(&host, &input).unwrap_err();
    assert_eq!(refused.code, "rules::board::refused");
    let input = json!({"path": "/docs", "stage": "orient", "agent": "lead"});
    for_stage(&host, &input).unwrap();
}

#[test]
fn check_and_for_path_never_list_a_stage_rule_and_list_an_ordinary_twin() {
    let ordinary = json!({
        "$type": "rules/rule/v1",
        "requirement": "Docs say what exists.",
        "purpose": "Readers trust them.",
        "paths": ["docs/*"],
    });
    let mut registered = ordinary.clone();
    registered["show"] = json!({"stage": "orient", "agent": "builder"});
    registered["provides"] = json!({"provide": "context", "file": "/docs/a.md"});
    let (_scenario, host) = board(&[
        ("docs/a.md", "A.\n"),
        ("rules/imposed/staged.json", &registered.to_string()),
        ("rules/imposed/ordinary.json", &ordinary.to_string()),
    ]);
    let checked = check(&host, &json!({})).unwrap();
    let stated: Vec<&Value> = (checked["stated"].as_array().unwrap().iter())
        .map(|each| &each["rule"])
        .collect();
    assert_eq!(stated, [&json!("/rules/imposed/ordinary")], "{checked}");
    let applying = for_path(&host, &json!({"path": "/docs/a.md"})).unwrap();
    let rules: Vec<&Value> = (applying["rules"].as_array().unwrap().iter())
        .map(|each| &each["rule"])
        .collect();
    assert_eq!(rules, [&json!("/rules/imposed/ordinary")]);
}
