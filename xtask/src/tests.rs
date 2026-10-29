//! Test support shared by every module, and tests of the whole lint.

use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use crate::files::Root;
use crate::report::{CheckKind, Report};

/// A repository in a temporary directory. Its root is a subdirectory, so a
/// test can also write a file just outside it.
pub(crate) struct Fixture {
    _directory: TempDir,
    root: PathBuf,
}

impl Fixture {
    pub(crate) fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("repo");
        create(&root, None);
        Self {
            _directory: directory,
            root,
        }
    }

    /// Writes `text` to `relative`, which may climb out of the root with `..`.
    pub(crate) fn write(&self, relative: &str, text: &str) -> &Self {
        create(&self.root.join(relative), Some(text));
        self
    }

    pub(crate) fn path(&self) -> &Path {
        &self.root
    }

    pub(crate) fn root(&self) -> Root {
        Root::open(&self.root).unwrap()
    }
}

#[expect(
    clippy::disallowed_methods,
    reason = "test fixtures are the one writer of test files"
)]
fn create(path: &Path, text: Option<&str>) {
    match text {
        None => fs::create_dir_all(path).unwrap(),
        Some(text) => {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
    }
}

/// `count` words of filler text.
pub(crate) fn words(count: usize) -> String {
    vec!["word"; count].join(" ")
}

/// A lint.toml with small budgets, so tests can cross them cheaply.
pub(crate) const CONFIG: &str = r#""$type" = "board/lint-config/v1"
[context]
entry = ["AGENTS.md", "CLAUDE.md"]
max_words = 10
[docs_index]
file = "DOCS.md"
max_words = 5
[docs]
directory = "docs"
max_words = 20
[plan]
file = "PLAN.md"
max_words = 6
[plan_leaves]
directory = "plan"
max_words = 8
[source]
max_lines = 3
[expect]
count = 0
[signing]
literals = ["ns/sign/v1"]
owners = ["src/sign.rs"]
"#;

/// The controls that record `CONFIG`'s word budgets, by path.
pub(crate) const CONTROLS: [(&str, &str); 5] = [
    (
        "controls/budgets/context.json",
        r#"{"applies_to": ["/AGENTS.md", "/CLAUDE.md"], "requirement": "At most 10 words."}"#,
    ),
    (
        "controls/budgets/docs_index.json",
        r#"{"applies_to": ["/DOCS.md"], "requirement": "At most 5 words."}"#,
    ),
    (
        "controls/budgets/docs.json",
        r#"{"paths": ["docs/*.md"], "requirement": "At most 20 words."}"#,
    ),
    (
        "controls/budgets/plan.json",
        r#"{"applies_to": ["/PLAN.md"], "requirement": "At most 6 words."}"#,
    ),
    (
        "controls/budgets/plan_leaves.json",
        r#"{"paths": ["plan/*.md"], "requirement": "At most 8 words."}"#,
    ),
];

fn within_every_budget() -> Fixture {
    let fixture = Fixture::new();
    for (path, text) in CONTROLS {
        fixture.write(path, text);
    }
    fixture
        .write("lint.toml", CONFIG)
        .write("CLAUDE.md", "@AGENTS.md")
        .write("AGENTS.md", &words(9))
        .write("DOCS.md", &words(5))
        .write("docs/a.md", &words(20))
        .write("PLAN.md", &format!("[a](plan/a.md) {}", words(5)))
        .write("plan/a.md", &words(8))
        .write("src/lib.rs", "fn a() {}\n");
    fixture
}

/// The kinds of the checks in `report` that failed, in report order.
fn failed(report: &Report) -> Vec<CheckKind> {
    report
        .checks
        .iter()
        .filter(|check| !check.passed())
        .map(|check| check.kind)
        .collect()
}

#[test]
fn the_lint_passes_a_repository_within_every_budget() {
    let report = crate::lint(within_every_budget().path()).unwrap();
    assert!(report.passed(), "{}", crate::report::render(&report));
}

#[test]
fn the_lint_fails_a_repository_over_one_budget_and_names_that_check() {
    let fixture = within_every_budget();
    fixture.write("docs/b.md", "one");
    let report = crate::lint(fixture.path()).unwrap();
    assert_eq!(failed(&report), [CheckKind::Docs]);
}

#[test]
fn the_lint_fails_a_repository_whose_plan_is_over_budget_and_names_the_plan_check() {
    let fixture = within_every_budget();
    fixture.write("PLAN.md", &format!("[a](plan/a.md) {}", words(6)));
    let report = crate::lint(fixture.path()).unwrap();
    assert_eq!(failed(&report), [CheckKind::Plan]);
    let text = crate::report::render(&report);
    assert!(text.contains("plan.max_words"), "{text}");
}

#[test]
fn the_lint_fails_a_repository_whose_plan_leaf_is_over_budget_and_passes_its_twin() {
    let fixture = within_every_budget();
    fixture.write("plan/a.md", &words(9));
    let report = crate::lint(fixture.path()).unwrap();
    assert_eq!(failed(&report), [CheckKind::PlanLeaf]);
    let text = crate::report::render(&report);
    assert!(text.contains("xtask::budget::over_limit"), "{text}");
    assert!(text.contains("plan_leaves.max_words"), "{text}");

    let twin = within_every_budget();
    twin.write("plan/a.md", &words(8));
    assert!(crate::lint(twin.path()).unwrap().passed());
}

#[test]
fn the_lint_fails_a_repository_with_a_plan_leaf_that_plan_md_does_not_link_and_passes_its_twin() {
    let fixture = within_every_budget();
    fixture.write("plan/b.md", "one");
    let report = crate::lint(fixture.path()).unwrap();
    assert_eq!(failed(&report), [CheckKind::PlanLinks]);
    let text = crate::report::render(&report);
    assert!(text.contains("xtask::plan::index_differs"), "{text}");
    assert!(text.contains("plan/b.md"), "{text}");

    let twin = within_every_budget();
    twin.write("plan/b.md", "one").write(
        "PLAN.md",
        &format!("[a](plan/a.md) [b](plan/b.md) {}", words(3)),
    );
    assert!(crate::lint(twin.path()).unwrap().passed());
}

#[test]
fn the_lint_fails_a_repository_whose_control_disagrees_with_lint_toml_and_passes_its_twin() {
    let fixture = within_every_budget();
    fixture.write(
        "controls/budgets/plan.json",
        r#"{"applies_to": ["/PLAN.md"], "requirement": "At most 7 words."}"#,
    );
    let report = crate::lint(fixture.path()).unwrap();
    assert!(!report.passed());
    assert_eq!(failed(&report), []);
    let text = crate::report::render(&report);
    assert!(
        text.contains(
            "FAIL  controls   xtask::control::differs: controls/budgets/plan.json: requirement = "
        ),
        "{text}"
    );

    let twin = within_every_budget();
    twin.write(
        "controls/budgets/plan.json",
        r#"{"applies_to": ["/PLAN.md"], "requirement": "At most 6 words."}"#,
    );
    assert!(crate::lint(twin.path()).unwrap().passed());
}

#[test]
fn the_lint_refuses_a_repository_without_lint_toml() {
    let fixture = Fixture::new();
    let error = crate::lint(fixture.path()).unwrap_err();
    assert_eq!(error.code(), "xtask::files::unreadable");
}

#[test]
fn this_repository_passes_its_own_lint() {
    let report = crate::lint(Path::new(crate::ROOT)).unwrap();
    assert!(report.passed(), "{}", crate::report::render(&report));
}
