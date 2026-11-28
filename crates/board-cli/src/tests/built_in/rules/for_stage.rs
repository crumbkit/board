//! `board rules for-stage` through the binary: a board with registrations
//! under /rules/imposed/ gives one stage's skills and context to one kind
//! of agent, refuses an unknown stage or kind, and keeps every stage rule
//! out of `rules check` and `rules for-path`.

use super::*;

/// A rule registered against `stage` and `agent`, providing `provide`
/// from `file`, with `fields` added.
fn registration(stage: &str, agent: &str, provide: &str, file: &str, fields: &Value) -> String {
    record(
        json!({
            "$type": "rules/rule/v1",
            "requirement": "An agent at this stage uses what the rule provides.",
            "purpose": "Every repository works the same way.",
            "show": {"stage": stage, "agent": agent},
            "provides": {"provide": provide, "file": file},
        }),
        fields,
    )
}

/// A board holding a skill, a context file, a registration of each for
/// the builder's orient stage, one for the reviewer, one whose file is
/// missing, and an ordinary rule on the docs.
fn registered() -> Fixture {
    let board = board();
    board
        .write("docs/a.md", "A.\n")
        .write("skills/tdd/SKILL.md", "# TDD\n")
        .write("context/orient.md", "Read the plan first.\n")
        .write(
            "rules/imposed/orient/tdd.json",
            &registration(
                "orient",
                "builder",
                "skill",
                "/skills/tdd/SKILL.md",
                &json!({"summary": "Write each test first."}),
            ),
        )
        .write(
            "rules/imposed/orient/context.json",
            &registration("orient", "builder", "context", "/context/orient.md", &json!({})),
        )
        .write(
            "rules/imposed/orient/gone.json",
            &registration("orient", "builder", "skill", "/skills/gone/SKILL.md", &json!({})),
        )
        .write(
            "rules/imposed/review.json",
            &registration("review", "reviewer", "skill", "/skills/tdd/SKILL.md", &json!({})),
        )
        .write(
            "rules/docs.json",
            &record(
                json!({"$type": "rules/rule/v1", "requirement": "Docs say what exists.", "purpose": "A test."}),
                &json!({"paths": ["docs/*"]}),
            ),
        );
    board
}

/// The output of `board --json rules for-stage` with `flags`, which exits 0.
fn for_stage(board: &Fixture, flags: &[&str]) -> Value {
    let arguments: Vec<&str> = ["--json", "rules", "for-stage"]
        .into_iter()
        .chain(flags.iter().copied())
        .collect();
    let ran = board.run(&arguments);
    assert_eq!(ran.code, 0, "{}{}", text(&ran.stdout), text(&ran.stderr));
    json(&ran)["data"]["output"].clone()
}

#[test]
fn for_stage_gives_the_builders_orient_skills_and_context_and_refuses_the_missing_file_alone() {
    let board = registered();
    let output = for_stage(
        &board,
        &["--path", "/", "--stage", "orient", "--agent", "builder"],
    );
    assert_eq!(
        output,
        json!({
            "stage": "orient",
            "agent": "builder",
            "skills": [{"name": "tdd", "file": "/skills/tdd/SKILL.md", "summary": "Write each test first."}],
            "context": [{"rule": "/rules/imposed/orient/context", "text": "Read the plan first."}],
            "left_out": [],
            "refused": [{
                "rule": "/rules/imposed/orient/gone",
                "code": "rules::stage::file_missing",
                "message": output["refused"][0]["message"],
            }],
        })
    );
    let reviewer = for_stage(
        &board,
        &["--path", "/", "--stage", "review", "--agent", "reviewer"],
    );
    assert_eq!(reviewer["skills"][0]["name"], "tdd");
    assert_eq!(reviewer["refused"], json!([]));
}

#[test]
fn an_unknown_stage_or_agent_kind_exits_1_with_its_code_and_a_listed_one_exits_0() {
    let board = registered();
    for flags in [
        ["--path", "/", "--stage", "deploy", "--agent", "builder"],
        ["--path", "/", "--stage", "orient", "--agent", "designer"],
    ] {
        let refusal = board.refusal("rules.for-stage", &flags);
        assert_eq!(refusal["code"], "rules::stage::unknown", "{refusal}");
    }
    let ran = board.run(&[
        "rules",
        "for-stage",
        "--path",
        "/",
        "--stage",
        "land",
        "--agent",
        "lead",
    ]);
    assert_eq!(ran.code, 0, "{}", text(&ran.stderr));
}

#[test]
fn a_path_board_cannot_resolve_exits_1_as_refused_and_one_it_resolves_exits_0() {
    let board = registered();
    let flags = [
        "--path",
        "/repos/hangar",
        "--stage",
        "orient",
        "--agent",
        "builder",
    ];
    let refusal = board.refusal("rules.for-stage", &flags);
    assert_eq!(refusal["code"], "rules::board::refused", "{refusal}");
    for_stage(
        &board,
        &["--path", "/docs", "--stage", "orient", "--agent", "builder"],
    );
}

#[test]
fn rules_check_and_for_path_omit_every_stage_rule_and_keep_the_ordinary_one() {
    let board = registered();
    board.write(
        "rules/imposed/docs-orient.json",
        &registration(
            "orient",
            "builder",
            "context",
            "/docs/a.md",
            &json!({"paths": ["docs/*"]}),
        ),
    );
    assert_eq!(applying(&board, "/docs/a.md"), ["/rules/docs"]);
    let checked = board.call("rules.check", &["--path", "/docs"]);
    let stated: Vec<&Value> = (checked["stated"].as_array().unwrap().iter())
        .map(|each| &each["rule"])
        .collect();
    assert_eq!(stated, [&json!("/rules/docs")], "{checked}");
}

#[test]
fn the_board_admits_a_stage_rule_and_refuses_one_with_an_unknown_stage() {
    let board = registered();
    board.write(
        "rules/imposed/deploy.json",
        &registration(
            "deploy",
            "builder",
            "skill",
            "/skills/tdd/SKILL.md",
            &json!({}),
        ),
    );
    let checked = board.checked("/rules/imposed/");
    assert_eq!(checked["/rules/imposed/deploy"].as_deref(), INVALID);
    assert_eq!(checked["/rules/imposed/orient/tdd"], None);
}

#[test]
fn help_for_for_stage_shows_its_example_commands() {
    let board = board();
    let help = board.run(&["rules", "for-stage", "-h"]);
    assert_eq!(help.code, 0, "{}", text(&help.stderr));
    let help = text(&help.stdout);
    for example in [
        "board rules for-stage --path /repos/app --stage orient --agent builder",
        "board rules for-stage --path / --stage review --agent reviewer",
    ] {
        assert!(help.contains(example), "{example}: {help}");
    }
}
