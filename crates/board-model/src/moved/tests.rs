use serde_json::json;

use super::*;

fn moved(from: &str, to: &str) -> Result<Moved, MovedError> {
    Moved::new(from.parse().unwrap(), to.parse().unwrap())
}

#[test]
fn a_path_under_either_prefix_swaps_to_the_other_and_one_under_neither_does_not() {
    let moved = moved("/controls", "/rules").unwrap();
    assert_eq!(
        moved.swap(&["controls", "board", "x"]),
        Some(vec!["rules".to_owned(), "board".to_owned(), "x".to_owned()])
    );
    assert_eq!(
        moved.swap(&["rules", "x"]),
        Some(vec!["controls".to_owned(), "x".to_owned()])
    );
    assert_eq!(moved.swap(&["controlsx", "x"]), None);
    assert_eq!(moved.swap(&["tasks", "controls"]), None);
    assert_eq!(moved.swap::<&str>(&[]), None);
}

#[test]
fn a_prefix_of_several_segments_swaps_only_whole_segments() {
    let moved = moved("/a/controls", "/rules").unwrap();
    assert_eq!(
        moved.swap(&["a", "controls", "x"]),
        Some(vec!["rules".to_owned(), "x".to_owned()])
    );
    assert_eq!(moved.swap(&["a", "x"]), None);
}

#[test]
fn a_root_or_overlapping_prefix_is_refused_and_two_apart_are_admitted() {
    assert_eq!(moved("/", "/rules"), Err(MovedError::Root));
    assert!(matches!(
        moved("/rules/old", "/rules"),
        Err(MovedError::Overlap { .. })
    ));
    assert!(matches!(
        moved("/rules", "/rules"),
        Err(MovedError::Overlap { .. })
    ));
    assert!(moved("/rules-old", "/rules").is_ok());
}

#[test]
fn a_moved_prefix_reads_from_json_with_both_fields_and_no_others() {
    let read = |value| serde_json::from_value::<Moved>(value);
    let moved = read(json!({"from": "/controls", "to": "/rules"})).unwrap();
    assert_eq!(moved.from().as_str(), "/controls");
    assert!(read(json!({"from": "/controls", "to": "/rules", "x": 1})).is_err());
    assert!(read(json!({"from": "/", "to": "/rules"})).is_err());
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
