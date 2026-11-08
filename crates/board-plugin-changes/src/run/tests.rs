//! The agent's report is read from its last line of stdout, and a run
//! names at most one stop.

use super::*;

#[test]
fn the_cost_and_duration_are_read_from_the_last_line_only_when_it_is_a_json_object() {
    let printed = "working\n{\"total_cost_usd\": 0.42, \"duration_ms\": 1200}\n\n";
    assert_eq!(reported(printed), (Some(0.42), Some(1200)));
    let earlier = "{\"total_cost_usd\": 0.42}\ndone\n";
    assert_eq!(reported(earlier), (None, None));
    assert_eq!(reported(""), (None, None));
}

fn asked(through: Option<&str>, until: Option<&str>) -> Input {
    serde_json::from_value(json!({"path": "/changes/x", "through": through, "until": until}))
        .expect("the input is valid")
}

#[test]
fn both_stops_are_refused_and_either_one_alone_is_admitted() {
    let refused = asked_stop(&asked(Some("plan"), Some("review"))).expect_err("two stops");
    assert_eq!(refused.code, "changes::run::two_stops");
    assert_eq!(
        asked_stop(&asked(Some("plan"), None)).expect("one stop"),
        Some(Stop::Through("plan".to_owned()))
    );
    assert_eq!(
        asked_stop(&asked(None, Some("review"))).expect("one stop"),
        Some(Stop::Until("review".to_owned()))
    );
    assert_eq!(asked_stop(&asked(None, None)).expect("no stop"), None);
}

#[test]
fn the_stopping_text_names_the_budget_only_when_one_is_given() {
    let without = stopping("/changes/x", "/repos/x", None);
    assert!(without.contains("changes stop --path /changes/x --reason reached"));
    assert!(!without.contains("US dollars"));
    assert!(stopping("/changes/x", "/repos/x", Some(2.5)).contains("at most 2.5 US dollars"));
}

#[test]
fn a_run_time_is_the_same_instant_as_its_stamp_to_the_second() {
    let at = SystemTime::UNIX_EPOCH + Duration::from_micros(1_793_967_334_421_737);
    assert_eq!(stamp(at), "20261106T121534421737Z");
    assert_eq!(time_of(at), "2026-11-06T12:15:34Z");
}
