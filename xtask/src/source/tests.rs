use super::*;
use crate::tests::Fixture;

#[test]
fn counts_expect_attributes_including_inner_ones_and_cfg_attr() {
    let text = "#![expect(clippy::a, reason = \"x\")]\n#[expect(clippy::b)]\nfn f() {}\n    #[cfg_attr(test, expect(clippy::c))]\n";
    assert_eq!(expect_attributes(text), 3);
}

#[test]
fn counts_a_cfg_attr_expect_written_over_several_lines_once() {
    let text = "#[cfg_attr(\n    target_os = \"macos\",\n    expect(clippy::a, reason = \"x\")\n)]\nfn f() {}\n";
    assert_eq!(expect_attributes(text), 1);
}

#[test]
fn ignores_other_attributes_comments_and_the_expect_method() {
    let text = "#[derive(Debug)]\n#[cfg_attr(test, derive(Clone))]\n// #[expect(clippy::a)]\n/// #[expect(clippy::b)]\nlet x = y.expect(\"proven\");\n";
    assert_eq!(expect_attributes(text), 0);
}

#[test]
fn a_reason_cannot_end_the_attribute_or_fake_an_expect() {
    let text = "#[cfg_attr(test, allow(clippy::a), doc = \"]expect(\")]\n#[expect(clippy::b, reason = \"a ] inside\")]\n";
    assert_eq!(expect_attributes(text), 1);
}

fn run(fixture: &Fixture, max_lines: usize, count: usize) -> [Check; 2] {
    checks(
        &fixture.root(),
        &SourceBudget { max_lines },
        &ExpectBudget { count },
    )
    .unwrap()
}

#[test]
fn a_file_at_the_line_limit_passes_and_one_line_over_fails() {
    let fixture = Fixture::new();
    fixture.write("src/a.rs", "1\n2\n3\n");
    assert!(run(&fixture, 3, 0)[0].passed());
    assert!(!run(&fixture, 2, 0)[0].passed());
}

#[test]
fn the_expect_count_must_match_exactly() {
    let fixture = Fixture::new();
    fixture.write(
        "src/a.rs",
        "#[expect(clippy::a, reason = \"x\")]\nfn f() {}\n",
    );
    assert!(run(&fixture, 10, 1)[1].passed());
    assert!(!run(&fixture, 10, 0)[1].passed());
    assert!(!run(&fixture, 10, 2)[1].passed());
}

#[test]
fn test_files_are_exempt_from_the_line_limit_and_the_count() {
    let fixture = Fixture::new();
    let long = "#[expect(clippy::a)]\n".repeat(10);
    fixture
        .write("src/a.rs", "fn f() {}\n")
        .write("src/a/tests.rs", &long)
        .write("tests/b.rs", &long);
    let [lines, expects] = run(&fixture, 1, 0);
    assert!(lines.passed());
    assert!(expects.passed());
}

#[test]
fn target_and_hidden_directories_are_not_source() {
    let fixture = Fixture::new();
    let long = "x\n".repeat(10);
    fixture
        .write("target/a.rs", &long)
        .write(".git/b.rs", &long)
        .write("src/c.rs", "x\n");
    assert!(run(&fixture, 1, 0)[0].passed());
}
