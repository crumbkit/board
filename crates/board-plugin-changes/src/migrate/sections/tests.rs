use super::{cut, read_section};

fn lines(text: &str) -> Vec<String> {
    text.lines().map(str::to_owned).collect()
}

#[test]
fn a_label_followed_by_list_items_is_unread_and_its_twin_followed_by_paths_is_a_group() {
    let (_, unread) = read_section(&lines("Built on:\n- /notes/a.md: the base.\n"));
    assert_eq!(unread, ["Built on:"]);
    let (groups, unread) = read_section(&lines("Unrelated:\n/notes/a.md\n/notes/b.md\n"));
    assert!(unread.is_empty(), "{unread:?}");
    assert_eq!(
        groups,
        [(
            vec!["/notes/a.md".to_owned(), "/notes/b.md".to_owned()],
            "Unrelated".to_owned()
        )]
    );
}

#[test]
fn a_blank_line_run_inside_a_fence_survives_and_a_run_at_the_cut_collapses_to_one() {
    let text =
        "# T\n\n```\nfirst\n\n\nsecond\n```\n\n\n### Considered\n\n- /a: b.\n\n## Outcome\n\nB.\n";
    assert_eq!(
        cut(text).kept,
        "# T\n\n```\nfirst\n\n\nsecond\n```\n\n## Outcome\n\nB.\n"
    );
}

#[test]
fn a_blank_line_run_outside_any_cut_is_left_as_the_plan_wrote_it() {
    let text = "# T\n\n\n## Problem\n\nA.\n\n### Considered\n\n- /a: b.\n";
    assert_eq!(cut(text).kept, "# T\n\n\n## Problem\n\nA.\n");
}
