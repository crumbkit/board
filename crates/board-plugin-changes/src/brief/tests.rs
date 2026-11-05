//! The pure parts of the brief: fitting rules into the word limit and the
//! input of `controls.for-paths`. The capability runs through the binary in
//! board-cli's changes tests.

use super::{Rule, fill, fit_rules, for_paths_input};

fn rule(control: &str, summary: &str, matched: Option<&[&str]>) -> Rule {
    Rule {
        control: control.to_owned(),
        summary: summary.to_owned(),
        matched: matched.map(|paths| paths.iter().map(|each| (*each).to_owned()).collect()),
        matched_left_out: None,
    }
}

#[test]
fn the_input_of_for_paths_is_written_as_python_wrote_it() {
    let paths = ["/repos/x/**".to_owned(), "/repos/x/a'b".to_owned()];
    assert_eq!(
        for_paths_input(&paths),
        r#"{"paths": ["/repos/x/**", "/repos/x/a'b"]}"#
    );
}

#[test]
fn rules_that_matched_some_paths_come_first_and_each_group_is_by_control() {
    let found = [
        rule("/c/b", "B.", None),
        rule("/c/a", "A.", None),
        rule("/c/z", "Z.", Some(&["/repos/x/a"])),
    ];
    let (rules, named, unnamed, _) = fit_rules(&found, 100);
    let order: Vec<&str> = rules.iter().map(|each| each.control.as_str()).collect();
    assert_eq!(order, ["/c/z", "/c/a", "/c/b"]);
    assert_eq!(named, Vec::<String>::new());
    assert_eq!(unnamed, 0);
}

#[test]
fn a_rule_too_long_to_fit_leaves_room_for_the_shorter_rules_after_it() {
    let long = "word ".repeat(20);
    let found = [rule("/c/a", &long, None), rule("/c/b", "Short.", None)];
    let (rules, left_out, spent) = fill(&found, 5);
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].control, "/c/b");
    assert_eq!(left_out, ["/c/a"]);
    assert_eq!(spent, 2);
}

#[test]
fn every_rule_gets_its_summary_before_any_rule_gets_its_matched_paths() {
    let found = [
        rule("/c/a", "A.", Some(&["/p/1", "/p/2"])),
        rule("/c/b", "B.", None),
    ];
    let (rules, left_out, spent) = fill(&found, 5);
    assert_eq!(left_out, Vec::<String>::new());
    assert_eq!(rules.len(), 2);
    assert!(
        rules[0].matched.is_none(),
        "the matched paths would pass the room"
    );
    assert_eq!(spent, 4);
}
