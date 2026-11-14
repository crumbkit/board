//! The build gate reads the latest recheck of each point of an attached
//! understanding check, so a missed recheck passes once a later recheck of
//! its point matches.

use serde_json::{Value, json};

use super::{Given, HEAD, RUN, UNDERSTOOD, given, planned, run, signed, understanding};

/// One recheck, as `(point, matches)`; `None` leaves the field out.
type Pair = (Option<&'static str>, Option<bool>);

/// A recheck of `point` that `matches`.
fn of(point: &'static str, matches: bool) -> Pair {
    (Some(point), Some(matches))
}

/// Rechecks built from `(point, matches)` pairs, in order.
fn from_pairs(pairs: &[Pair]) -> Value {
    (pairs.iter())
        .map(|(point, matches)| {
            let mut recheck =
                json!({"kind": "edge", "question": "Q?", "answer": "A.", "design": "A."});
            if let Some(point) = point {
                recheck["point"] = json!(point);
            }
            if let Some(matches) = matches {
                recheck["matches"] = json!(matches);
            }
            recheck
        })
        .collect()
}

/// A change in build whose attached understanding checks are `records`,
/// each named by its path, all signed off, against a flow that wants
/// `wanted` points when it is not two.
fn built(records: &[(&'static str, &[Pair])], wanted: Option<u32>) -> Given {
    let stored = planned("No UI change: it is headless.", "One record per call.");
    let mut attached = vec![(RUN, run(&json!({})))];
    let mut paths = Vec::new();
    for (path, pairs) in records {
        attached.push((
            *path,
            understanding(&json!({"rechecks": from_pairs(pairs)})),
        ));
        paths.push(*path);
    }
    attached.push(("/d/s", signed(&paths)));
    let given =
        (given("build").prose().plan(Some(&stored)).records(attached)).with(json!({"head": HEAD}));
    match wanted {
        Some(wanted) => given.rechecks(wanted),
        None => given,
    }
}

fn single(pairs: &[Pair]) -> Given {
    built(&[(UNDERSTOOD, pairs)], None)
}

const MISSED: &str = "the understanding check at /changes/t/understanding/data has points whose \
     latest recheck does not match the plan: ";
const FEW: &str = "the understanding check at /changes/t/understanding/data holds rechecks of \
     fewer than ";

#[test]
fn a_point_whose_latest_recheck_misses_refuses_and_is_named() {
    let found = single(&[of("a", true), of("b", true), of("a", false)]).check("rechecks");
    assert_eq!(found.0, "missing");
    assert!(
        found.1.starts_with(&format!("{MISSED}a; settle")),
        "{}",
        found.1
    );
    let both = single(&[of("a", false), of("b", false)]).check("rechecks");
    assert!(
        both.1.starts_with(&format!("{MISSED}a, b; settle")),
        "{}",
        both.1
    );
}

#[test]
fn two_rechecks_of_one_point_count_once() {
    let found = single(&[of("a", true), of("a", true)]).check("rechecks");
    assert_eq!(found.0, "missing");
    assert!(
        found.1.starts_with(&format!("{FEW}two points")),
        "{}",
        found.1
    );
}

#[test]
fn a_latest_recheck_that_does_not_say_it_matches_is_a_miss() {
    let found = single(&[of("a", true), of("b", true), (Some("a"), None)]).check("rechecks");
    assert_eq!(found.0, "missing");
    assert!(found.1.starts_with(&format!("{MISSED}a;")), "{}", found.1);
}

#[test]
fn a_flow_that_wants_three_points_refuses_two() {
    let pairs = [of("a", true), of("b", true), of("b", true)];
    let found = built(&[(UNDERSTOOD, &pairs)], Some(3)).check("rechecks");
    assert_eq!(found.0, "missing");
    assert!(
        found.1.starts_with(&format!("{FEW}three points")),
        "{}",
        found.1
    );
}

#[test]
fn a_missed_recheck_passes_once_a_later_recheck_of_its_point_matches() {
    let found = single(&[of("a", false), of("b", true), of("a", false), of("a", true)]);
    assert_eq!(found.check("rechecks").0, "pass");
}

#[test]
fn a_recheck_with_no_point_refuses_and_names_the_record() {
    let found = single(&[of("a", true), of("b", true), (None, Some(true))]).check("rechecks");
    assert_eq!(found.0, "missing");
    assert_eq!(
        found.1,
        "the understanding check at /changes/t/understanding/data holds a recheck with no \
         point; give each of its rechecks a point in the board that holds it, then update the \
         mount"
    );
    // A recheck with no point comes before a missed point, and a missed
    // point before too few points.
    let no_point = single(&[(None, Some(false)), of("a", false)]).check("rechecks");
    assert!(
        no_point.1.contains("holds a recheck with no point"),
        "{}",
        no_point.1
    );
    let missed = single(&[of("a", false)]).check("rechecks");
    assert!(missed.1.starts_with(MISSED), "{}", missed.1);
}

#[test]
fn the_second_attached_understanding_check_can_refuse() {
    let second = "/changes/t/understanding/second";
    let good = [of("a", true), of("b", true)];
    let bad = [of("a", true), of("b", false)];
    let found = built(&[(UNDERSTOOD, &good), (second, &bad)], None).check("rechecks");
    assert_eq!(found.0, "missing");
    assert!(
        found.1.starts_with(&format!(
            "the understanding check at {second} has points whose latest recheck"
        )),
        "{}",
        found.1
    );
}
