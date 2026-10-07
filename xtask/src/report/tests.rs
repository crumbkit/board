use super::*;

fn check(kind: CheckKind, limit: Limit, amounts: &[usize]) -> Check {
    let parts = amounts
        .iter()
        .enumerate()
        .map(|(index, amount)| Part::new(format!("f{index}"), *amount))
        .collect();
    Check::total(kind, limit, parts)
}

#[test]
fn a_passing_line_shows_usage_and_the_largest_parts_first() {
    let report = Report {
        checks: vec![check(CheckKind::Docs, Limit::AtMost(10), &[2, 5])],
        controls: Vec::new(),
    };
    let text = render(&report);
    assert!(text.starts_with("pass  docs"), "{text}");
    assert!(text.contains("7 / 10"), "{text}");
    assert!(text.contains("f1 5, f0 2"), "{text}");
}

#[test]
fn a_failing_line_names_its_code_and_what_to_change() {
    let report = Report {
        checks: vec![check(CheckKind::DocsIndex, Limit::AtMost(1), &[2])],
        controls: Vec::new(),
    };
    let text = render(&report);
    assert!(text.starts_with("FAIL"), "{text}");
    assert!(text.contains("xtask::budget::over_limit"), "{text}");
    assert!(text.contains("docs_index.max_words"), "{text}");
}

#[test]
fn an_expect_count_above_or_below_says_which_way_to_move_it() {
    let above = render(&Report {
        checks: vec![check(CheckKind::Expect, Limit::Exactly(1), &[2])],
        controls: Vec::new(),
    });
    let below = render(&Report {
        checks: vec![check(CheckKind::Expect, Limit::Exactly(1), &[])],
        controls: Vec::new(),
    });
    assert!(above.contains("set expect.count = 2"), "{above}");
    assert!(below.contains("lower expect.count to 0"), "{below}");
}

#[test]
fn a_long_list_of_parts_is_summarised() {
    let report = Report {
        checks: vec![check(CheckKind::Docs, Limit::AtMost(100), &[1; 7])],
        controls: Vec::new(),
    };
    assert!(render(&report).contains("and 2 more"));
}

#[test]
fn a_failure_shows_its_code_and_every_cause() {
    let error = crate::files::read(std::path::Path::new("/nonexistent/board-lint")).unwrap_err();
    let text = render_failure(error.code(), &error);
    assert!(
        text.starts_with("error: xtask::files::unreadable: cannot read /nonexistent/board-lint: "),
        "{text}"
    );
}
