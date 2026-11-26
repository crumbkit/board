use std::ffi::OsString;
use std::time::SystemTime;

use board_ops::{Plugin, Settings, Variables, With};
use board_scenario::Scenario;
use serde_json::{Map, Value, json};

use crate::Review;
use crate::git;

/// Settings that run git from `/usr/bin` or `/bin`, started in `caller`.
fn settings(caller: Option<&Scenario>) -> Settings {
    Settings {
        now: SystemTime::UNIX_EPOCH,
        day_start: None,
        utc_offset: 0,
        caller: caller.map(|scenario| scenario.workdir().to_path_buf()),
        environment: Variables(vec![("PATH".to_owned(), OsString::from("/usr/bin:/bin"))]),
        cache_home: None,
    }
}

/// What the review plugin's one fill makes of `report`.
fn filled(report: &Value, settings: &Settings) -> Result<Option<Value>, String> {
    let [fill] = Review.fills() else {
        panic!("review declares one fill");
    };
    assert_eq!(
        (fill.record_type, fill.field),
        ("review/report/v1", "patch_ids")
    );
    let With::Derived(derive) = fill.with else {
        panic!("patch ids are derived");
    };
    let report: Map<String, Value> = serde_json::from_value(report.clone()).unwrap();
    derive(&report, settings)
}

/// A repository with a base commit and two commits after it, the second
/// changing only whitespace, so their patch ids differ.
fn repository() -> (Scenario, String, String) {
    let scenario = Scenario::new();
    let base = scenario.commit(&[("a.txt", "one\n")]);
    scenario.commit(&[("a.txt", "one\ntwo\n")]);
    let head = scenario.commit(&[("a.txt", "one\n two\n")]);
    (scenario, base.to_string(), head.to_string())
}

#[test]
fn a_reports_patch_ids_are_what_review_run_records_for_its_range() {
    let (scenario, base, head) = repository();
    let settings = settings(Some(&scenario));
    let report = json!({"range": format!("{base}..{head}"), "findings": [], "verdict": "approve"});
    let Some(Value::Array(ids)) = filled(&report, &settings).unwrap() else {
        panic!("patch ids are filled");
    };
    let recorded = git::with(&settings, scenario.workdir())
        .covered(&base, &head)
        .unwrap();
    assert_eq!(ids, recorded.iter().map(|id| json!(id)).collect::<Vec<_>>());
    assert_eq!(ids.len(), 2);
    assert_ne!(ids[0], ids[1]);
}

#[test]
fn a_report_with_no_range_is_left_without_patch_ids() {
    let (scenario, ..) = repository();
    let report = json!({"subject": "/tasks/a", "findings": [], "verdict": "approve"});
    assert_eq!(filled(&report, &settings(Some(&scenario))), Ok(None));
}

#[test]
fn a_range_the_checkout_lacks_is_not_filled_and_says_which_commit_is_missing() {
    let (scenario, base, _) = repository();
    let missing = "c".repeat(40);
    let report =
        json!({"range": format!("{base}..{missing}"), "findings": [], "verdict": "approve"});
    let why = filled(&report, &settings(Some(&scenario))).unwrap_err();
    assert!(
        why.contains(&format!("'{missing}' names no commit")),
        "{why}"
    );
    assert!(
        why.ends_with(
            "run the put from a checkout of the repository that holds the range, or give patch_ids"
        ),
        "{why}"
    );
}

#[test]
fn a_put_with_no_checkout_does_not_fill_patch_ids() {
    let (_, base, head) = repository();
    let report = json!({"range": format!("{base}..{head}"), "findings": [], "verdict": "approve"});
    let why = filled(&report, &settings(None)).unwrap_err();
    assert!(why.contains("no checkout"), "{why}");
}
