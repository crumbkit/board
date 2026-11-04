//! When a repeating task comes back, and what completing a task writes.
//! These were `test_repeat.py`.

use serde_json::json;

use super::*;

fn day(text: &str) -> Date {
    Date::parse(text).unwrap()
}

#[test]
fn the_next_occurrence_is_the_first_after_the_date_with_a_month_day_clamped_to_its_last() {
    let cases = [
        ("daily", "2026-12-31", "2027-01-01"),
        ("weekdays", "2026-10-23", "2026-10-26"),
        ("weekdays", "2026-10-24", "2026-10-26"),
        ("weekdays", "2026-10-25", "2026-10-26"),
        ("weekdays", "2026-10-26", "2026-10-27"),
        ("weekly-mon", "2026-10-26", "2026-11-02"),
        ("weekly-sun", "2026-10-26", "2026-11-01"),
        ("monthly-15", "2026-10-14", "2026-10-15"),
        ("monthly-15", "2026-10-15", "2026-11-15"),
        ("monthly-31", "2027-01-31", "2027-02-28"),
        ("monthly-31", "2027-02-28", "2027-03-31"),
        ("monthly-31", "2026-12-31", "2027-01-31"),
        ("annually-02-29", "2027-01-01", "2027-02-28"),
        ("annually-02-29", "2027-02-28", "2028-02-29"),
        ("annually-10-25", "2026-10-25", "2027-10-25"),
    ];
    for (rule, after, then) in cases {
        assert_eq!(
            following(rule, day(after)),
            day(then),
            "{rule} after {after}"
        );
    }
}

const TODAY: &str = "2026-10-28";

fn completing(record: &Value) -> Vec<(&'static str, Option<String>)> {
    completed(record.as_object().unwrap(), day(TODAY))
}

fn deferred(to: &str) -> Vec<(&'static str, Option<String>)> {
    vec![("defer", Some(to.to_owned()))]
}

#[test]
fn a_task_without_repeat_is_done() {
    assert_eq!(
        completing(&json!({"status": "open"})),
        vec![("status", Some("done".to_owned()))]
    );
}

#[test]
fn a_repeating_task_is_deferred_to_its_next_occurrence_never_earlier_than_its_defer_and_stays_open()
{
    assert_eq!(
        completing(&json!({"repeat": "daily"})),
        deferred("2026-10-29")
    );
    assert_eq!(
        completing(&json!({"repeat": "daily", "defer": "soon"})),
        deferred("2026-10-29"),
        "no reviewed"
    );
    let soon = json!({"repeat": "daily", "defer": "soon", "reviewed": "2026-10-27"});
    assert_eq!(
        completing(&soon),
        deferred("2026-11-17"),
        "soon ends 20 days after reviewed"
    );
    let missed = json!({"repeat": "daily", "defer": "soon", "reviewed": "2026-09-01"});
    assert_eq!(
        completing(&missed),
        deferred("2026-09-22"),
        "a missed horizon"
    );
    let later = json!({"repeat": "daily", "defer": "2026-11-05"});
    assert_eq!(completing(&later), deferred("2026-11-06"));
    let later = json!({"repeat": "daily", "defer": "2026-11-05", "ephemeral": true});
    assert_eq!(completing(&later), deferred("2026-11-06"));
}

#[test]
fn a_missed_occurrence_comes_back_one_by_one_unless_the_task_is_ephemeral() {
    let missed = json!({"repeat": "daily", "defer": "2026-10-25"});
    assert_eq!(
        completing(&missed),
        deferred("2026-10-26"),
        "the next missed day"
    );
    let moment = json!({"repeat": "daily", "defer": "2026-10-25T09:00:00+02:00"});
    assert_eq!(completing(&moment), deferred("2026-10-26"));
    let ephemeral = json!({"repeat": "daily", "defer": "2026-10-25", "ephemeral": true});
    assert_eq!(completing(&ephemeral), deferred("2026-10-29"), "no backlog");
}
