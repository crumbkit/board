//! `ci`: the check runs GitHub reports for a change's head become a run of
//! the change's suite only when every one passed, and the run and head are
//! attached to the change. Each refusal writes nothing and has a passing
//! twin that is admitted.

use super::desk::Desk;
use super::merge::{CHANGE, HEAD};
use super::*;

const SUITE: &str = "/suites/checks";
const RUN: &str = "/suites/checks/runs/20261119T1057Z-lever-0123456";

/// A desk whose personal board enables changes and runs.
fn desk() -> Desk {
    let desk = Desk::new();
    for plugin in ["changes", "runs"] {
        let enabled = desk.run_in(desk.personal.workdir(), &["plugin", "enable", plugin], &[]);
        assert_eq!(enabled.code, 0, "{}", text(&enabled.stderr));
    }
    desk
}

/// Writes the suite and a change at [`HEAD`] that names it, with `fields`
/// over the change; a null field is left out.
fn lever(desk: &Desk, fields: &Value) {
    let suite = json!({"$type": "runs/suite/v1", "method": "CI on GitHub."});
    desk.write("suites/checks.json", &suite.to_string());
    desk.write("changes/lever/plan.md", "# Lever\n");
    let plan = json!({
        "$type": "changes/plan/v1",
        "text": "/changes/lever/plan.md",
        "uses": [],
        "records": [],
        "answers": {"ux": "none", "data": "none", "lifecycle": "none"},
    });
    desk.write("changes/lever/plans/1.json", &plan.to_string());
    let mut change = json!({
        "$type": "changes/change/v2",
        "title": "Add a lever",
        "mount": "/repos/widgets",
        "phase": "build",
        "touched": "2026-11-19",
        "head": HEAD,
        "suite": SUITE,
        "plan": "/changes/lever/plans/1",
    });
    for (key, value) in fields.as_object().unwrap() {
        if value.is_null() {
            change.as_object_mut().unwrap().remove(key);
        } else {
            change[key] = value.clone();
        }
    }
    desk.write("changes/lever.json", &change.to_string());
}

/// One check run as GitHub reports it, cut to what the plugin reads.
fn check(name: &str, status: &str, conclusion: Option<&str>, minute: u8) -> Value {
    json!({
        "name": name,
        "status": status,
        "conclusion": conclusion,
        "started_at": format!("2026-11-19T10:{minute:02}:18Z"),
        "completed_at": conclusion.map(|_| format!("2026-11-19T10:{:02}:42Z", minute + 5)),
    })
}

/// What `gh api .../check-runs` prints for `runs`.
fn answers(desk: &Desk, runs: &[Value]) {
    let answer = json!({"total_count": runs.len(), "check_runs": runs});
    desk.gh_answers(&answer, 0, "");
}

/// Three check runs that passed, the last one skipped.
fn passing() -> Vec<Value> {
    vec![
        check("lint", "completed", Some("success"), 52),
        check("test", "completed", Some("success"), 51),
        check("docs", "completed", Some("skipped"), 50),
    ]
}

/// Whether any run of the suite was written.
fn wrote_a_run(desk: &Desk) -> bool {
    desk.personal.workdir().join("suites/checks/runs").exists()
}

/// The refusal `ci` gives for `runs`, after which nothing is written; then
/// the passing twin is admitted.
fn refused_then_admitted(runs: &[Value]) -> Value {
    let desk = desk();
    lever(&desk, &json!({}));
    answers(&desk, runs);
    let (ran, document) = desk.call("ci", &["--change", CHANGE]);
    assert_eq!(ran.code, 1, "{document}");
    assert!(!wrote_a_run(&desk));
    assert_eq!(desk.record(CHANGE).unwrap().get("head"), Some(&json!(HEAD)));
    answers(&desk, &passing());
    assert_eq!(desk.output("ci", &["--change", CHANGE])["run"], RUN);
    document
}

#[test]
fn ci_writes_a_passing_run_of_the_changes_suite_from_its_check_runs_and_attaches_it() {
    let desk = desk();
    lever(&desk, &json!({}));
    answers(&desk, &passing());
    let output = desk.output("ci", &["--change", CHANGE]);
    assert_eq!(
        output,
        json!({"run": RUN, "head": HEAD, "change": CHANGE, "checks": 3})
    );
    let endpoint = format!("repos/octo/widgets/commits/{HEAD}/check-runs?per_page=100");
    assert_eq!(desk.gh_argv(), ["api", &endpoint]);
    assert_eq!(
        desk.record(RUN).unwrap(),
        json!({
            "$type": "runs/run/v1",
            "suite": SUITE,
            "by": {"program": "board"},
            "written_by": "github/ci",
            "started": "2026-11-19T10:50:18Z",
            "finished": "2026-11-19T10:57:42Z",
            "scope": "full",
            "dirty": false,
            "results": [
                {"case": "lint", "outcome": "pass"},
                {"case": "test", "outcome": "pass"},
                {"case": "docs", "outcome": "skipped", "note": "GitHub concluded skipped."},
            ],
            "versions": {"widgets": HEAD},
            "measures": {"checks": 3, "skipped": 1},
            "setup": format!(
                "Read by board github ci from the check runs GitHub reports for octo/widgets at \
                 {HEAD}. Each result is one check run, named as GitHub names it."
            ),
            "summary": "All 3 check runs completed with success, skipped or neutral.",
        })
    );
    let plan = desk.record("/changes/lever/plans/1").unwrap();
    assert_eq!(plan["records"], json!([RUN]));
    assert_eq!(desk.record(CHANGE).unwrap()["head"], HEAD);
}

#[test]
fn ci_with_a_check_still_running_is_refused_as_pending_and_its_finished_twin_is_admitted() {
    let mut runs = passing();
    runs[1] = check("test", "in_progress", None, 51);
    runs.push(check("deny", "queued", None, 53));
    let refused = refused_then_admitted(&runs);
    assert_eq!(refused["code"], "github::ci::pending");
    assert_eq!(refused["fields"]["checks"], json!(["test", "deny"]));
    let message = refused["message"].as_str().unwrap();
    assert!(
        message.contains("test (in_progress), deny (queued)"),
        "{message}"
    );
    let hint = refused["recovery"]["text"].as_str().unwrap();
    assert!(
        hint.contains("board github ci --change /changes/lever"),
        "{hint}"
    );
}

#[test]
fn ci_with_a_failed_check_is_refused_as_failed_and_its_passing_twin_is_admitted() {
    let mut runs = passing();
    runs[0] = check("lint", "completed", Some("failure"), 52);
    runs[1] = check("test", "completed", Some("timed_out"), 51);
    runs.push(check("deny", "queued", None, 53));
    let refused = refused_then_admitted(&runs);
    assert_eq!(refused["code"], "github::ci::failed");
    assert_eq!(refused["fields"]["checks"], json!(["lint", "test"]));
    let message = refused["message"].as_str().unwrap();
    assert!(
        message.contains("lint (failure), test (timed_out)"),
        "{message}"
    );
    let hint = refused["recovery"]["text"].as_str().unwrap();
    assert!(hint.contains("--head <sha>"), "{hint}");
}

#[test]
fn ci_on_a_head_with_no_check_runs_is_refused_and_its_twin_with_runs_is_admitted() {
    let refused = refused_then_admitted(&[]);
    assert_eq!(refused["code"], "github::ci::none");
    assert_eq!(refused["fields"]["checks"], json!([]));
    let message = refused["message"].as_str().unwrap();
    assert!(message.contains(HEAD), "{message}");
}

#[test]
fn ci_of_a_change_with_no_suite_is_refused_without_gh_and_one_with_a_suite_is_admitted() {
    let desk = desk();
    lever(&desk, &json!({"suite": null}));
    answers(&desk, &passing());
    let (ran, document) = desk.call("ci", &["--change", CHANGE]);
    assert_eq!(
        (ran.code, &document["code"]),
        (1, &json!("github::ci::change"))
    );
    assert_eq!(desk.gh_calls(), 0);
    lever(&desk, &json!({}));
    assert_eq!(desk.output("ci", &["--change", CHANGE])["run"], RUN);
}
