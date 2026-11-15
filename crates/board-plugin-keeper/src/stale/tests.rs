//! Each stale rule at its boundary: the day that is stale, and the day
//! before it that is not.

use super::*;

fn today() -> Date {
    "2026-11-15".parse().unwrap()
}

fn change(path: &str, phase: &str, touched: Option<&str>) -> ChangeFacts {
    ChangeFacts {
        path: path.to_owned(),
        title: format!("Title of {path}"),
        phase: phase.to_owned(),
        prototype: false,
        verdict: None,
        touched: touched.map(str::to_owned),
        task: None,
        mount: None,
        plan: None,
    }
}

fn task(path: &str, reviewed: Option<&str>) -> TaskFacts {
    TaskFacts {
        path: path.to_owned(),
        title: format!("Title of {path}"),
        reviewed: reviewed.map(str::to_owned),
    }
}

fn paths(items: &[Item]) -> Vec<&str> {
    items.iter().map(|item| item.path.as_str()).collect()
}

#[test]
fn a_change_in_build_is_stale_after_three_days_and_not_on_the_third() {
    let found = stale(
        today(),
        &[
            change("/changes/old", "build", Some("2026-11-11")),
            change("/changes/edge", "build", Some("2026-11-12")),
            change("/changes/review", "review", Some("2026-10-01")),
        ],
        &[],
    );
    assert_eq!(paths(&found.changes), ["/changes/old"]);
    assert_eq!(found.changes[0].days, Some(4));
    assert_eq!(found.changes[0].since.as_deref(), Some("2026-11-11"));
}

#[test]
fn a_prototype_is_stale_when_it_has_no_verdict_and_no_news_for_three_days() {
    let mut quiet = change("/changes/quiet", "build", Some("2026-11-01"));
    let mut judged = change("/changes/judged", "build", Some("2026-11-01"));
    let mut fresh = change("/changes/fresh", "build", Some("2026-11-13"));
    for each in [&mut quiet, &mut judged, &mut fresh] {
        each.prototype = true;
    }
    judged.verdict = Some("It holds.".to_owned());
    let found = stale(today(), &[quiet, judged, fresh], &[]);
    assert_eq!(paths(&found.prototypes), ["/changes/quiet"]);
    assert!(found.changes.is_empty(), "a prototype is not a build");
}

#[test]
fn a_task_is_stale_after_fourteen_days_with_no_change_and_never_when_a_change_names_it() {
    let mut named = change("/changes/c", "plan", Some("2026-11-15"));
    named.task = Some("/tasks/named.md".to_owned());
    let found = stale(
        today(),
        &[named],
        &[
            task("/tasks/named.md", Some("2026-01-01")),
            task("/tasks/old.md", Some("2026-10-31")),
            task("/tasks/edge.md", Some("2026-11-01")),
            task("/tasks/never.md", None),
        ],
    );
    assert_eq!(paths(&found.tasks), ["/tasks/never.md", "/tasks/old.md"]);
    assert_eq!(found.tasks[0].days, None);
    assert_eq!(found.tasks[1].days, Some(15));
}

#[test]
fn a_day_that_does_not_read_is_listed_with_no_age() {
    let found = stale(
        today(),
        &[change("/changes/odd", "build", Some("last week"))],
        &[],
    );
    assert_eq!(paths(&found.changes), ["/changes/odd"]);
    assert_eq!(found.changes[0].days, None);
}
