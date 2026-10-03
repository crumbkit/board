use super::*;
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

#[test]
fn the_docs_index_at_its_budget_passes_and_one_word_over_fails() {
    let fixture = Fixture::new();
    fixture.write("DOCS.md", &words(4));
    let budget = |max_words| FileBudget {
        file: "DOCS.md".to_owned(),
        max_words,
    };
    assert!(docs_index(&fixture.root(), &budget(4)).unwrap().passed());
    assert!(!docs_index(&fixture.root(), &budget(3)).unwrap().passed());
}

#[test]
fn docs_sums_only_the_files_the_glob_matches() {
    let fixture = Fixture::new();
    fixture
        .write("docs/a.md", &words(2))
        .write("docs/b.md", &words(3))
        .write("docs/notes.txt", &words(100))
        .write("docs/deeper/c.md", &words(100));
    let budget = GlobBudget {
        glob: "docs/*.md".to_owned(),
        max_words: 5,
    };
    let check = docs(&fixture.root(), &budget).unwrap();
    assert_eq!(check.used, 5);
    assert!(check.passed());
}

#[test]
fn docs_with_a_recursive_glob_counts_subdirectories() {
    let fixture = Fixture::new();
    fixture
        .write("docs/a.md", &words(2))
        .write("docs/deeper/c.md", &words(3));
    let budget = GlobBudget {
        glob: "docs/**/*.md".to_owned(),
        max_words: 4,
    };
    let check = docs(&fixture.root(), &budget).unwrap();
    assert_eq!(check.used, 5);
    assert!(!check.passed());
}
