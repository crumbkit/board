use super::{Section, with_section};

const PLAN: &str = "# T\n\n## Problem\n\nIt hangs.\n";

#[test]
fn a_section_the_text_lacks_is_appended_under_its_own_heading() {
    assert_eq!(
        with_section(PLAN, Section::Proves, "It stops."),
        Some(format!("{PLAN}\n## What the tests prove\n\nIt stops.\n"))
    );
}

#[test]
fn a_text_with_no_final_newline_gets_one_before_the_appended_section() {
    assert_eq!(
        with_section("# T", Section::DoesNotProve, " Retries. \n"),
        Some("# T\n\n## What the tests do not prove\n\nRetries.\n".to_owned())
    );
}

#[test]
fn an_empty_heading_is_filled_in_place_and_every_other_line_is_kept() {
    let text = format!("{PLAN}\n## What the tests prove\n\n## Notes\n\nN.\n");
    assert_eq!(
        with_section(&text, Section::Proves, "It stops."),
        Some(format!(
            "{PLAN}\n## What the tests prove\n\nIt stops.\n\n## Notes\n\nN.\n"
        ))
    );
}

#[test]
fn a_heading_that_holds_text_is_refused_and_an_empty_one_is_filled() {
    let held = format!("{PLAN}\n### what the tests prove\n\nIt stops.\n");
    assert_eq!(with_section(&held, Section::Proves, "More."), None);
    let empty = format!("{PLAN}\n### what the tests prove\n");
    assert_eq!(
        with_section(&empty, Section::Proves, "More."),
        Some(format!("{PLAN}\n### what the tests prove\n\nMore.\n"))
    );
}

#[test]
fn a_heading_inside_a_code_fence_is_not_filled_and_the_section_is_appended() {
    let text = format!("{PLAN}\n```\n## What the tests prove\n```\n");
    assert_eq!(
        with_section(&text, Section::Proves, "It stops."),
        Some(format!("{text}\n## What the tests prove\n\nIt stops.\n"))
    );
}
