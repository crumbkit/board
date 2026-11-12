use super::*;

fn moved(from: &str, to: &str) -> Option<Moved> {
    Moved::new(from.parse().unwrap(), to.parse().unwrap())
}

fn swap(from: &str, to: &str, segments: &[&str]) -> Option<String> {
    swapped(&[moved(from, to).unwrap()], segments).map(|swapped| swapped.join("/"))
}

#[test]
fn a_path_under_either_prefix_swaps_to_the_other_and_one_under_neither_does_not() {
    let swap = |segments: &[&str]| swap("/controls", "/rules", segments);
    assert_eq!(
        swap(&["controls", "board", "x"]).as_deref(),
        Some("rules/board/x")
    );
    assert_eq!(swap(&["rules", "x"]).as_deref(), Some("controls/x"));
    assert_eq!(swap(&["controlsx", "x"]), None);
    assert_eq!(swap(&["tasks", "controls"]), None);
    assert_eq!(swap(&[]), None);
}

#[test]
fn a_prefix_of_several_segments_swaps_only_whole_segments() {
    let swap = |segments: &[&str]| swap("/a/controls", "/rules", segments);
    assert_eq!(swap(&["a", "controls", "x"]).as_deref(), Some("rules/x"));
    assert_eq!(swap(&["a", "x"]), None);
    assert_eq!(swap(&["a"]), None);
}

#[test]
fn a_root_or_overlapping_prefix_is_refused_and_two_apart_are_admitted() {
    assert_eq!(moved("/", "/rules"), None);
    assert_eq!(moved("/rules/old", "/rules"), None);
    assert_eq!(moved("/rules", "/rules"), None);
    assert!(moved("/rules-old", "/rules").is_some());
}

#[test]
fn the_first_moved_prefix_that_applies_is_swapped() {
    let all = [moved("/a", "/b").unwrap(), moved("/c", "/d").unwrap()];
    assert_eq!(
        swapped(&all, &["d", "x"]),
        Some(vec!["c".to_owned(), "x".to_owned()])
    );
    assert_eq!(swapped(&all, &["e"]), None);
}
