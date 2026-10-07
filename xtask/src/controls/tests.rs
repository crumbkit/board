use super::*;
use crate::config::Config;
use crate::tests::{CONFIG, CONTROLS, Fixture};

/// The controls that record `CONFIG`'s word budgets, with the one at `path`
/// replaced by `text`, or left out when `text` is `None`.
fn controls_with(path: &str, text: Option<&str>) -> Fixture {
    let fixture = Fixture::new();
    fixture.write("lint.toml", CONFIG);
    for (control, original) in CONTROLS {
        if control != path {
            fixture.write(control, original);
        }
    }
    if let Some(text) = text {
        fixture.write(path, text);
    }
    fixture
}

/// Every divergence `check` finds, as the lint prints it after its code.
fn messages(fixture: &Fixture) -> Vec<String> {
    let config = Config::parse(CONFIG).unwrap();
    check(&fixture.root(), &config)
        .unwrap()
        .iter()
        .map(ToString::to_string)
        .collect()
}

const NONE: [&str; 0] = [];
const PLAN: &str = "controls/budgets/plan.json";
const DOCS: &str = "controls/budgets/docs.json";
const CONTEXT: &str = "controls/budgets/context.json";

#[test]
fn controls_that_record_every_word_budget_agree_with_lint_toml() {
    assert_eq!(messages(&controls_with("", None)), NONE);
}

#[test]
fn a_control_named_after_no_lint_toml_section_diverges_and_its_twin_named_after_one_agrees() {
    let plan = r#"{"applies_to": ["/PLAN.md"], "requirement": "At most 6 words."}"#;
    let fixture = controls_with(PLAN, None);
    fixture.write("controls/budgets/plans.json", plan);
    assert_eq!(
        messages(&fixture),
        [
            "controls/budgets/plans.json: file name = plans, but lint.toml has no [plans] word budget; rename the control after a word budget, or remove it",
            "controls/budgets/plan.json: file = absent, but lint.toml [plan] sets max_words = 6; add the control, naming the same files and stating that number in its requirement",
        ]
    );
    let twin = controls_with(PLAN, None);
    twin.write(PLAN, plan);
    assert_eq!(messages(&twin), NONE);
}

#[test]
fn a_word_budget_without_a_control_diverges_and_names_the_file_to_add() {
    let fixture = controls_with("controls/budgets/docs_index.json", None);
    assert_eq!(
        messages(&fixture),
        [
            "controls/budgets/docs_index.json: file = absent, but lint.toml [docs_index] sets max_words = 5; add the control, naming the same files and stating that number in its requirement"
        ]
    );
    assert_eq!(messages(&controls_with("", None)), NONE);
}

#[test]
fn a_control_whose_paths_differ_from_the_glob_diverges_and_its_matching_twin_agrees() {
    let fixture = controls_with(
        DOCS,
        Some(r#"{"paths": ["docs/**/*.md"], "requirement": "At most 20 words."}"#),
    );
    assert_eq!(
        messages(&fixture),
        [
            r#"controls/budgets/docs.json: applies_to and paths = {"docs/**/*.md"}, but lint.toml [docs] names {"docs/*.md"}; change the control to name the same files"#
        ]
    );
    let twin = controls_with(
        DOCS,
        Some(r#"{"paths": ["docs/*.md"], "requirement": "At most 20 words."}"#),
    );
    assert_eq!(messages(&twin), NONE);
}

#[test]
fn a_control_missing_one_entry_file_diverges_and_its_complete_twin_agrees() {
    let fixture = controls_with(
        CONTEXT,
        Some(r#"{"applies_to": ["/AGENTS.md"], "requirement": "At most 10 words."}"#),
    );
    assert_eq!(messages(&fixture).len(), 1);
    let twin = controls_with(
        CONTEXT,
        Some(r#"{"applies_to": ["/AGENTS.md", "/CLAUDE.md"], "requirement": "At most 10 words."}"#),
    );
    assert_eq!(messages(&twin), NONE);
}

#[test]
fn a_requirement_that_states_another_number_diverges_and_its_twin_with_max_words_agrees() {
    let fixture = controls_with(
        PLAN,
        Some(r#"{"applies_to": ["/PLAN.md"], "requirement": "At most 7 words."}"#),
    );
    assert_eq!(
        messages(&fixture),
        [
            r#"controls/budgets/plan.json: requirement = "At most 7 words.", but lint.toml [plan] sets max_words = 6; make that the requirement's only number, or change lint.toml in a change that says why"#
        ]
    );
    let twin = controls_with(
        PLAN,
        Some(r#"{"applies_to": ["/PLAN.md"], "requirement": "At most 6 words."}"#),
    );
    assert_eq!(messages(&twin), NONE);
}

#[test]
fn a_requirement_that_states_max_words_beside_another_number_diverges_and_its_twin_agrees() {
    let fixture = controls_with(
        PLAN,
        Some(r#"{"applies_to": ["/PLAN.md"], "requirement": "At most 12 words, up from 6."}"#),
    );
    assert_eq!(messages(&fixture).len(), 1);
    assert!(
        messages(&fixture)[0].starts_with("controls/budgets/plan.json: requirement = "),
        "{:?}",
        messages(&fixture)
    );
    let twin = controls_with(
        PLAN,
        Some(r#"{"applies_to": ["/PLAN.md"], "requirement": "At most 6 words."}"#),
    );
    assert_eq!(messages(&twin), NONE);
}

#[test]
fn a_control_that_carries_a_rule_diverges_and_its_twin_without_one_agrees() {
    let fixture = controls_with(
        PLAN,
        Some(
            r#"{"applies_to": ["/PLAN.md"], "requirement": "At most 6 words.", "rule": {"budget": "words", "max": 6}}"#,
        ),
    );
    assert_eq!(
        messages(&fixture),
        [
            r#"controls/budgets/plan.json: rule = {"budget":"words","max":6}, but lint.toml [plan] enforces this budget; remove rule, because a budget control only records lint.toml"#
        ]
    );
    let twin = controls_with(
        PLAN,
        Some(r#"{"applies_to": ["/PLAN.md"], "requirement": "At most 6 words."}"#),
    );
    assert_eq!(messages(&twin), NONE);
}

#[test]
fn a_requirement_may_write_its_number_with_thousands_separators() {
    assert_eq!(numbers("PLAN.md holds at most 1,075 words."), [1075]);
    assert_eq!(numbers("At most 1075 words, or 75 more."), [1075, 75]);
}

#[test]
fn a_control_without_a_requirement_stops_the_lint_and_its_twin_with_one_does_not() {
    let config = Config::parse(CONFIG).unwrap();
    let fixture = controls_with(DOCS, Some(r#"{"paths": ["docs/*.md"]}"#));
    let error = check(&fixture.root(), &config).unwrap_err();
    assert_eq!(error.code(), "xtask::control::invalid");
    assert!(
        error
            .to_string()
            .starts_with("controls/budgets/docs.json is not a control"),
        "{error}"
    );
    let twin = controls_with(
        DOCS,
        Some(r#"{"paths": ["docs/*.md"], "requirement": "At most 20 words."}"#),
    );
    assert!(check(&twin.root(), &config).is_ok());
}

#[test]
fn only_json_files_in_the_budgets_directory_are_read_as_controls() {
    let fixture = controls_with("", None);
    fixture.write("controls/budgets/README.md", "not a control");
    fixture.write("controls/budgets/old.json/notes.txt", "not a control");
    assert_eq!(messages(&fixture), NONE);
    fixture.write("controls/budgets/stray.json", CONTROLS[0].1);
    assert_ne!(messages(&fixture), NONE);
}
