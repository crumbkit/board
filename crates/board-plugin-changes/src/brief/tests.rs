//! The pure parts of the brief: fitting rules into the word limit and the
//! input of `rules.for-paths`. The capability runs through the binary in
//! board-cli's changes tests.

use super::{LeftOut, Reason, RuleLine, fill, fit_rules, for_paths_input};

fn rule(path: &str, summary: &str, matched: Option<&[&str]>) -> RuleLine {
    RuleLine {
        rule: path.to_owned(),
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
fn rules_that_matched_some_paths_come_first_and_each_group_is_by_rule() {
    let found = [
        rule("/c/b", "B.", None),
        rule("/c/a", "A.", None),
        rule("/c/z", "Z.", Some(&["/repos/x/a"])),
    ];
    let (rules, named, unnamed, _) = fit_rules(&found, &[], 100);
    let order: Vec<&str> = rules.iter().map(|each| each.rule.as_str()).collect();
    assert_eq!(order, ["/c/z", "/c/a", "/c/b"]);
    assert_eq!(named, Vec::<LeftOut>::new());
    assert_eq!(unnamed, 0);
}

#[test]
fn a_rule_too_long_to_fit_leaves_room_for_the_shorter_rules_after_it() {
    let long = "word ".repeat(20);
    let found = [rule("/c/a", &long, None), rule("/c/b", "Short.", None)];
    let (rules, left_out, spent) = fill(&found, 5);
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].rule, "/c/b");
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

#[test]
fn dropped_version_rules_are_named_after_the_rules_with_no_room_within_the_room() {
    let found = [
        rule("/c/a", "word word word.", None),
        rule("/c/b", "B.", None),
    ];
    let dropped = ["/c/v".to_owned()];
    let left = |rule: &str, reason| LeftOut {
        rule: rule.to_owned(),
        reason,
    };
    // Each entry is 8 words, "/c/a word word word." 4 and "/c/b B." 2.
    let (rules, named, unnamed, spent) = fit_rules(&found, &dropped, 14);
    assert_eq!(rules.len(), 2);
    assert_eq!(named, [left("/c/v", Reason::NoVersion)]);
    assert_eq!((unnamed, spent), (0, 14));
    // The words kept for the dropped entry leave no room for /c/a, which is
    // named first and leaves no room for the dropped entry.
    let (rules, named, unnamed, spent) = fit_rules(&found, &dropped, 10);
    let shown: Vec<&str> = rules.iter().map(|each| each.rule.as_str()).collect();
    assert_eq!(shown, ["/c/b"]);
    assert_eq!(named, [left("/c/a", Reason::NoRoom)]);
    assert_eq!((unnamed, spent), (1, 10));
}
