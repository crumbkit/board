//! `ci`: the check runs GitHub reports for a change's head become a run of
//! the change's suite only when every check run, check suite and commit
//! status passed and one check run succeeded, and the run and head are
//! attached to the change. Each refusal writes nothing and has a passing
//! twin that is admitted.

use super::desk::{Desk, put_file};
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

/// One commit status as GitHub reports it.
fn status(context: &str, state: &str) -> Value {
    json!({"context": context, "state": state})
}

/// Makes the stub gh answer `.../check-runs` with `runs`,
/// `.../check-suites` with `suites` and `.../status` with `statuses`, and
/// keep each endpoint it is asked in `gh-endpoints.txt`.
fn answers(desk: &Desk, runs: &[Value], suites: &[Value], statuses: &[Value]) {
    desk.gh_also(
        "echo \"$2\" >> \"$here/gh-endpoints.txt\"\n\
         case \"$2\" in\n\
         *check-suites*) cat \"$here/suites.json\"; exit 0;;\n\
         */status*) cat \"$here/status.json\"; exit 0;;\n\
         esac",
    );
    let answer = json!({"total_count": runs.len(), "check_runs": runs});
    desk.gh_answers(&answer, 0, "");
    let suites = json!({"total_count": suites.len(), "check_suites": suites});
    put_file(&desk.bin().join("suites.json"), &suites.to_string());
    // GitHub's combined state is pending when there are no statuses.
    let combined = json!({"state": "pending", "total_count": statuses.len(), "statuses": statuses});
    put_file(&desk.bin().join("status.json"), &combined.to_string());
}

/// Three check runs that passed, the last one skipped.
fn passing() -> Vec<Value> {
    vec![
        check("lint", "completed", Some("success"), 52),
        check("test", "completed", Some("success"), 51),
        check("docs", "completed", Some("skipped"), 50),
    ]
}

/// One check suite of GitHub Actions in `status`, concluded success once
/// completed.
fn suite(status: &str) -> Vec<Value> {
    let conclusion = (status == "completed").then_some("success");
    concluded(status, conclusion)
}

/// One check suite of GitHub Actions in `status` with `conclusion`.
fn concluded(status: &str, conclusion: Option<&str>) -> Vec<Value> {
    vec![app_suite(
        "github-actions",
        "GitHub Actions",
        status,
        conclusion,
        1,
    )]
}

/// One check suite of the app `slug` named `name`, created on 2026-11-19,
/// with `runs` check runs.
fn app_suite(slug: &str, name: &str, status: &str, conclusion: Option<&str>, runs: usize) -> Value {
    json!({
        "status": status,
        "conclusion": conclusion,
        "app": {"slug": slug, "name": name},
        "created_at": "2026-11-19T10:49:00Z",
        "latest_check_runs_count": runs,
    })
}

/// Whether any run of the suite was written.
fn wrote_a_run(desk: &Desk) -> bool {
    desk.personal.workdir().join("suites/checks/runs").exists()
}

/// The refusal `ci` gives for `runs`, `suites` and `statuses`, after which
/// nothing is written; then the passing twin, with a completed suite and
/// no statuses, is admitted.
fn refused_then_admitted(runs: &[Value], suites: &[Value], statuses: &[Value]) -> Value {
    let desk = desk();
    lever(&desk, &json!({}));
    answers(&desk, runs, suites, statuses);
    let (ran, document) = desk.call("ci", &["--change", CHANGE]);
    assert_eq!(ran.code, 1, "{document}");
    assert!(!wrote_a_run(&desk));
    assert_eq!(desk.record(CHANGE).unwrap().get("head"), Some(&json!(HEAD)));
    answers(&desk, &passing(), &suite("completed"), &[]);
    assert_eq!(desk.output("ci", &["--change", CHANGE])["run"], RUN);
    document
}

#[test]
fn ci_writes_a_passing_run_of_the_changes_suite_from_its_check_runs_and_attaches_it() {
    let desk = desk();
    lever(&desk, &json!({}));
    answers(&desk, &passing(), &suite("completed"), &[]);
    let output = desk.output("ci", &["--change", CHANGE]);
    assert_eq!(
        output,
        json!({"run": RUN, "head": HEAD, "change": CHANGE, "checks": 3, "ignored": []})
    );
    let commit = format!("repos/octo/widgets/commits/{HEAD}");
    assert_eq!(
        desk.gh_file("gh-endpoints.txt"),
        format!(
            "{commit}/check-runs?per_page=100\n{commit}/check-suites?per_page=100\n{commit}/status?per_page=100\n"
        )
    );
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
            "summary": "Read by board github ci from the check runs GitHub reports.",
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
    let refused = refused_then_admitted(&runs, &suite("completed"), &[]);
    assert_eq!(refused["code"], "github::ci::pending");
    assert_eq!(refused["fields"]["checks"], json!(["test", "deny"]));
    let message = refused["message"].as_str().unwrap();
    assert!(
        message.contains("test (in_progress), deny (queued) have not finished"),
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
    let refused = refused_then_admitted(&runs, &suite("in_progress"), &[]);
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
    let refused = refused_then_admitted(&[], &[], &[]);
    assert_eq!(refused["code"], "github::ci::none");
    assert_eq!(refused["fields"]["checks"], json!([]));
    let message = refused["message"].as_str().unwrap();
    assert!(message.contains(HEAD), "{message}");
}

#[test]
fn ci_on_a_head_whose_check_runs_were_all_skipped_is_refused_as_none_and_its_twin_is_admitted() {
    let runs = [
        check("lint", "completed", Some("skipped"), 52),
        check("docs", "completed", Some("neutral"), 50),
    ];
    let refused = refused_then_admitted(&runs, &suite("completed"), &[]);
    assert_eq!(refused["code"], "github::ci::none");
    assert_eq!(refused["fields"]["checks"], json!(["lint", "docs"]));
}

#[test]
fn ci_while_a_check_suite_is_queued_is_refused_as_pending_though_every_check_run_passed() {
    let refused = refused_then_admitted(&passing(), &suite("queued"), &[]);
    assert_eq!(refused["code"], "github::ci::pending");
    assert_eq!(
        refused["fields"]["checks"],
        json!(["the check suite of GitHub Actions"])
    );
    let message = refused["message"].as_str().unwrap();
    assert!(
        message.contains("the check suite of GitHub Actions (queued) has not finished"),
        "{message}"
    );
}

#[test]
fn ci_with_a_check_suite_that_concluded_failure_and_no_check_runs_is_refused_and_its_twin_is_admitted()
 {
    let refused = refused_then_admitted(&[], &concluded("completed", Some("failure")), &[]);
    assert_eq!(refused["code"], "github::ci::failed");
    assert_eq!(
        refused["fields"]["checks"],
        json!(["the check suite of GitHub Actions"])
    );
    let message = refused["message"].as_str().unwrap();
    assert!(
        message.contains("the check suite of GitHub Actions (failure) did not pass"),
        "{message}"
    );
}

#[test]
fn ci_with_a_failed_commit_status_is_refused_as_failed_and_its_twin_is_admitted() {
    let statuses = [status("ci/old", "success"), status("ci/extra", "error")];
    let refused = refused_then_admitted(&passing(), &suite("completed"), &statuses);
    assert_eq!(refused["code"], "github::ci::failed");
    assert_eq!(refused["fields"]["checks"], json!(["ci/extra"]));
}

#[test]
fn ci_with_a_pending_commit_status_is_refused_as_pending_and_its_twin_is_admitted() {
    let statuses = [status("ci/extra", "pending")];
    let refused = refused_then_admitted(&passing(), &suite("completed"), &statuses);
    assert_eq!(refused["code"], "github::ci::pending");
    assert_eq!(refused["fields"]["checks"], json!(["ci/extra"]));
}

#[test]
fn ci_of_a_change_with_no_suite_is_refused_without_gh_and_one_with_a_suite_is_admitted() {
    let desk = desk();
    lever(&desk, &json!({"suite": null}));
    answers(&desk, &passing(), &suite("completed"), &[]);
    let (ran, document) = desk.call("ci", &["--change", CHANGE]);
    assert_eq!(
        (ran.code, &document["code"]),
        (1, &json!("github::ci::change"))
    );
    assert_eq!(desk.gh_calls(), 0);
    lever(&desk, &json!({}));
    assert_eq!(desk.output("ci", &["--change", CHANGE])["run"], RUN);
}

#[test]
fn ci_writes_the_run_past_an_old_app_suite_with_no_check_runs_and_its_twin_with_one_is_pending() {
    let desk = desk();
    lever(&desk, &json!({}));
    let mut suites = suite("completed");
    suites.push(app_suite("codecov", "Codecov", "queued", None, 1));
    answers(&desk, &passing(), &suites, &[]);
    let (ran, document) = desk.call("ci", &["--change", CHANGE]);
    assert_eq!(
        (ran.code, &document["code"]),
        (1, &json!("github::ci::pending"))
    );
    assert!(!wrote_a_run(&desk));
    suites[1]["latest_check_runs_count"] = json!(0);
    answers(&desk, &passing(), &suites, &[]);
    let output = desk.output("ci", &["--change", CHANGE]);
    assert_eq!(
        (&output["run"], &output["ignored"]),
        (&json!(RUN), &json!(["Codecov"]))
    );
    assert_eq!(
        desk.record(RUN).unwrap()["results"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
}
