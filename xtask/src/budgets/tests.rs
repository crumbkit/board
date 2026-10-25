use super::*;
use crate::report::{Report, render};
use crate::tests::{Fixture, words};

fn budget(max_words: usize) -> ContextBudget {
    ContextBudget {
        entry: vec!["AGENTS.md".to_owned(), "CLAUDE.md".to_owned()],
        max_words,
    }
}

fn labels(check: &Check) -> Vec<&str> {
    let mut labels: Vec<&str> = check.parts.iter().map(|part| part.label.as_str()).collect();
    labels.sort_unstable();
    labels
}

#[test]
fn context_counts_an_entry_that_another_entry_imports_once() {
    let fixture = Fixture::new();
    fixture
        .write("CLAUDE.md", "@AGENTS.md")
        .write("AGENTS.md", &words(9));
    let check = context(&fixture.root(), &budget(10)).unwrap();
    assert_eq!(check.used, 10);
    assert_eq!(labels(&check), ["AGENTS.md", "CLAUDE.md"]);
}

#[test]
fn context_at_its_budget_passes_and_one_word_over_fails() {
    let fixture = Fixture::new();
    fixture
        .write("CLAUDE.md", "@AGENTS.md")
        .write("AGENTS.md", &words(9));
    assert!(context(&fixture.root(), &budget(10)).unwrap().passed());
    assert!(!context(&fixture.root(), &budget(9)).unwrap().passed());
}

#[test]
fn context_follows_an_import_relative_to_the_importing_file() {
    let fixture = Fixture::new();
    fixture
        .write("CLAUDE.md", "@guide/a.md")
        .write("AGENTS.md", "")
        .write("guide/a.md", "@b.md")
        .write("guide/b.md", &words(3));
    let check = context(&fixture.root(), &budget(100)).unwrap();
    assert_eq!(
        labels(&check),
        ["AGENTS.md", "CLAUDE.md", "guide/a.md", "guide/b.md"]
    );
}

#[test]
fn context_ignores_an_import_that_names_no_file() {
    let fixture = Fixture::new();
    fixture
        .write("CLAUDE.md", "@missing.md @guide")
        .write("AGENTS.md", "")
        .write("guide/a.md", "");
    let check = context(&fixture.root(), &budget(100)).unwrap();
    assert_eq!(labels(&check), ["AGENTS.md", "CLAUDE.md"]);
}

#[test]
fn context_ignores_an_import_outside_the_repository() {
    let fixture = Fixture::new();
    fixture
        .write("CLAUDE.md", "@../outside.md @~/personal.md")
        .write("AGENTS.md", "")
        .write("../outside.md", &words(50));
    let check = context(&fixture.root(), &budget(100)).unwrap();
    assert_eq!(labels(&check), ["AGENTS.md", "CLAUDE.md"]);
}

#[test]
fn context_follows_five_hops_of_imports_and_no_more() {
    let fixture = Fixture::new();
    fixture.write("AGENTS.md", "@1.md").write("CLAUDE.md", "");
    for hop in 1..=6 {
        fixture.write(&format!("{hop}.md"), &format!("@{}.md", hop + 1));
    }
    let check = context(&fixture.root(), &budget(100)).unwrap();
    assert_eq!(
        labels(&check),
        [
            "1.md",
            "2.md",
            "3.md",
            "4.md",
            "5.md",
            "AGENTS.md",
            "CLAUDE.md"
        ]
    );
}

#[test]
fn context_survives_an_import_cycle() {
    let fixture = Fixture::new();
    fixture
        .write("AGENTS.md", "@CLAUDE.md")
        .write("CLAUDE.md", "@AGENTS.md");
    let check = context(&fixture.root(), &budget(100)).unwrap();
    assert_eq!(check.used, 2);
}

#[test]
fn context_refuses_a_missing_entry_file() {
    let fixture = Fixture::new();
    fixture.write("AGENTS.md", "");
    let error = context(&fixture.root(), &budget(100)).unwrap_err();
    assert_eq!(error.code(), "xtask::files::unreadable");
}

fn file_budget(file: &str, max_words: usize) -> FileBudget {
    FileBudget {
        file: file.to_owned(),
        max_words,
    }
}

#[test]
fn the_docs_index_at_its_budget_passes_and_one_word_over_fails() {
    let fixture = Fixture::new();
    fixture.write("DOCS.md", &words(4));
    let check = |max| {
        file(
            &fixture.root(),
            CheckKind::DocsIndex,
            &file_budget("DOCS.md", max),
        )
    };
    assert!(check(4).unwrap().passed());
    assert!(!check(3).unwrap().passed());
}

#[test]
fn the_plan_at_its_budget_passes_and_one_word_over_fails() {
    let fixture = Fixture::new();
    fixture.write("PLAN.md", &words(4));
    let check = |max| {
        file(
            &fixture.root(),
            CheckKind::Plan,
            &file_budget("PLAN.md", max),
        )
    };
    let within = check(4).unwrap();
    assert_eq!(within.kind, CheckKind::Plan);
    assert_eq!(within.parts, [Part::new("PLAN.md".to_owned(), 4)]);
    assert!(within.passed());
    assert!(!check(3).unwrap().passed());
}

#[test]
fn a_missing_plan_is_refused() {
    let fixture = Fixture::new();
    let error = file(&fixture.root(), CheckKind::Plan, &file_budget("PLAN.md", 4)).unwrap_err();
    assert_eq!(error.code(), "xtask::files::unreadable");
}

fn docs_budget(max_words: usize) -> DirectoryBudget {
    DirectoryBudget {
        directory: "docs".to_owned(),
        max_words,
    }
}

/// Each node's name and the words its own files hold, in report order.
fn nodes(checks: &[Check]) -> Vec<(&str, usize)> {
    checks
        .iter()
        .map(|check| (check.name(), check.used))
        .collect()
}

#[test]
fn a_docs_node_at_its_budget_passes() {
    let fixture = Fixture::new();
    fixture
        .write("docs/a.md", &words(3))
        .write("docs/b.md", &words(2));
    let checks = docs(&fixture.root(), &docs_budget(5)).unwrap();
    assert_eq!(nodes(&checks), [("docs", 5)]);
    assert!(checks[0].passed());
}

#[test]
fn a_docs_node_one_word_over_its_budget_fails_naming_the_node_and_where_detail_goes() {
    let fixture = Fixture::new();
    fixture
        .write("docs/a.md", &words(4))
        .write("docs/b.md", &words(2));
    let checks = docs(&fixture.root(), &docs_budget(5)).unwrap();
    assert_eq!(nodes(&checks), [("docs", 6)]);
    assert!(!checks[0].passed());
    let text = render(&Report {
        checks,
        controls: Vec::new(),
    });
    let advice = text.lines().nth(1).unwrap();
    assert_eq!(
        advice,
        "      xtask::budget::over_limit: docs holds 6 words in its own .md files, over \
         its limit of 5; move detail into a subdirectory of docs, or raise \
         docs.max_words in lint.toml in a change that says why"
    );
}

#[test]
fn a_subdirectory_is_its_own_node_and_its_words_do_not_count_toward_its_parent() {
    let fixture = Fixture::new();
    fixture
        .write("docs/a.md", &words(5))
        .write("docs/deeper/b.md", &words(1));
    let checks = docs(&fixture.root(), &docs_budget(5)).unwrap();
    assert_eq!(nodes(&checks), [("docs", 5), ("docs/deeper", 1)]);
    assert!(checks.iter().all(Check::passed));
}

#[test]
fn the_same_words_in_a_file_beside_the_parent_files_count_toward_the_parent() {
    let fixture = Fixture::new();
    fixture
        .write("docs/a.md", &words(5))
        .write("docs/b.md", &words(1));
    let checks = docs(&fixture.root(), &docs_budget(5)).unwrap();
    assert_eq!(nodes(&checks), [("docs", 6)]);
    assert!(!checks[0].passed());
}

#[test]
fn a_nested_node_over_its_budget_is_the_one_that_fails() {
    let fixture = Fixture::new();
    fixture
        .write("docs/a.md", &words(1))
        .write("docs/deeper/b.md", &words(1))
        .write("docs/deeper/still/c.md", &words(6));
    let checks = docs(&fixture.root(), &docs_budget(5)).unwrap();
    let failed: Vec<&str> = checks
        .iter()
        .filter(|check| !check.passed())
        .map(Check::name)
        .collect();
    assert_eq!(failed, ["docs/deeper/still"]);
}

#[test]
fn docs_counts_only_markdown_files() {
    let fixture = Fixture::new();
    fixture
        .write("docs/a.md", &words(2))
        .write("docs/notes.txt", &words(100));
    let checks = docs(&fixture.root(), &docs_budget(5)).unwrap();
    assert_eq!(nodes(&checks), [("docs", 2)]);
}

#[test]
fn an_empty_docs_directory_is_one_node_with_no_words() {
    let fixture = Fixture::new();
    fixture.write("docs/.keep", "");
    let checks = docs(&fixture.root(), &docs_budget(5)).unwrap();
    assert_eq!(nodes(&checks), [("docs", 0)]);
    assert!(checks[0].passed());
}

#[test]
fn docs_refuses_a_missing_directory() {
    let fixture = Fixture::new();
    let error = docs(&fixture.root(), &docs_budget(5)).unwrap_err();
    assert_eq!(error.code(), "xtask::files::unreadable");
}

fn leaves_budget(max_words: usize) -> DirectoryBudget {
    DirectoryBudget {
        directory: "plan".to_owned(),
        max_words,
    }
}

/// The plan's checks for a repository whose PLAN.md is `index`.
fn plan_checks(fixture: &Fixture, index: &str, max_words: usize) -> Vec<Check> {
    fixture.write("PLAN.md", index);
    plan_leaves(&fixture.root(), "PLAN.md", &leaves_budget(max_words)).unwrap()
}

/// The labels of the plan's index check, which comes last.
fn index_problems(checks: &[Check]) -> Vec<&str> {
    let index = checks.last().unwrap();
    assert_eq!(index.kind, CheckKind::PlanLinks);
    labels(index)
}

#[test]
fn a_plan_leaf_at_its_budget_passes_and_one_word_over_fails_naming_the_leaf() {
    let fixture = Fixture::new();
    fixture.write("plan/a.md", &words(3));
    let within = plan_checks(&fixture, "[a](plan/a.md)", 3);
    assert_eq!(nodes(&within), [("plan/a.md", 3), ("plan-links", 0)]);
    assert!(within.iter().all(Check::passed));

    let over = plan_checks(&fixture, "[a](plan/a.md)", 2);
    assert!(!over[0].passed());
    let text = render(&Report {
        checks: over,
        controls: Vec::new(),
    });
    assert!(text.contains("split plan/a.md"), "{text}");
}

#[test]
fn a_plan_leaf_that_plan_md_does_not_link_fails_and_its_linked_twin_passes() {
    let fixture = Fixture::new();
    fixture.write("plan/a.md", "");
    let unlinked = plan_checks(&fixture, "no links", 5);
    assert_eq!(
        index_problems(&unlinked),
        ["plan/a.md is not linked from PLAN.md"]
    );
    let linked = plan_checks(&fixture, "[a](plan/a.md#goal)", 5);
    assert!(index_problems(&linked).is_empty());
}

#[test]
fn a_plan_link_to_a_missing_leaf_fails_and_its_twin_naming_a_leaf_passes() {
    let fixture = Fixture::new();
    fixture.write("plan/a.md", "");
    let missing = plan_checks(&fixture, "[a](plan/a.md) [b](plan/b.md)", 5);
    assert_eq!(
        index_problems(&missing),
        ["PLAN.md links plan/b.md, which does not exist"]
    );
    let present = plan_checks(&fixture, "[a](plan/a.md) [c](docs/c.md)", 5);
    assert!(index_problems(&present).is_empty());
}

#[test]
fn a_plan_leaf_linked_twice_fails_and_its_twin_linked_once_passes() {
    let fixture = Fixture::new();
    fixture.write("plan/a.md", "");
    let twice = plan_checks(&fixture, "[a](plan/a.md) [again](plan/a.md)", 5);
    assert_eq!(index_problems(&twice), ["PLAN.md links plan/a.md 2 times"]);
    let once = plan_checks(&fixture, "[a](plan/a.md)", 5);
    assert!(index_problems(&once).is_empty());
}

#[test]
fn a_subdirectory_in_the_plan_directory_fails_and_its_flat_twin_passes() {
    let fixture = Fixture::new();
    fixture.write("plan/a.md", "").write("plan/deeper/b.md", "");
    assert_eq!(
        index_problems(&plan_checks(&fixture, "[a](plan/a.md)", 5)),
        ["plan/deeper is a subdirectory; the plan holds one flat file per milestone"]
    );
    let flat = Fixture::new();
    flat.write("plan/a.md", "").write("plan/b.md", "");
    let checks = plan_checks(&flat, "[a](plan/a.md) [b](plan/b.md)", 5);
    assert!(index_problems(&checks).is_empty());
}

#[test]
fn a_repository_without_a_plan_directory_has_no_leaves() {
    let fixture = Fixture::new();
    let checks = plan_checks(&fixture, "no links", 5);
    assert_eq!(nodes(&checks), [("plan-links", 0)]);
}
