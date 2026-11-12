//! `pr-create`, `pr-edit`, `comment` and `issue-create`: gh runs only after
//! the boundary passes what it would send.

use super::desk::{Desk, executable, put_file, remove};
use super::*;

const PULL_URL: &str = "https://github.com/Octo/Widgets/pull/12";
const ISSUE_URL: &str = "https://github.com/Octo/Widgets/issues/7";

/// The fields of a boundary refusal, once its codes are checked.
fn refused((ran, document): (Output, Value)) -> Value {
    assert_eq!(ran.code, 1, "{document}");
    assert_eq!(document["code"], "github::boundary::refused");
    assert_eq!(document["fields"]["code"], "boundary::check::failed");
    let hint = document["recovery"]["text"].as_str().unwrap();
    assert!(hint.starts_with("Next: "), "{hint}");
    document["fields"].clone()
}

/// The code of a call that failed.
fn failed((ran, document): (Output, Value)) -> (u8, Value) {
    (ran.code, document["code"].clone())
}

#[test]
fn pr_create_pushes_opens_the_pull_request_and_writes_its_record() {
    let desk = Desk::new();
    desk.git(&desk.checkout(), &["switch", "-q", "-c", "feature"]);
    let sha = desk.commit("Add a lever");
    desk.gh_answers(
        &json!(format!("Creating pull request\n{PULL_URL}\n")),
        0,
        "",
    );
    let flags = [
        "--title",
        "Add a lever",
        "--body",
        "Levers lift.",
        "--base",
        "main",
    ];
    let result = desk.output("pr-create", &flags);
    assert_eq!(desk.remote_head("feature"), Some(sha));
    assert_eq!(
        desk.gh_argv(),
        [
            "pr",
            "create",
            "--repo",
            "octo/widgets",
            "--head",
            "feature",
            "--title",
            "Add a lever",
            "--body-file",
            "-",
            "--base",
            "main"
        ]
    );
    assert_eq!(desk.gh_file("gh-stdin.txt"), "Levers lift.");
    assert_eq!(
        (&result["sent"], &result["number"], &result["path"]),
        (
            &json!(PULL_URL),
            &json!(12),
            &json!("/gh/octo/widgets/pulls/12")
        )
    );
    assert_eq!(
        desk.record("/gh/octo/widgets/pulls/12").unwrap(),
        json!({
            "$type": "github/pull/v1",
            "owner": "octo",
            "repo": "widgets",
            "number": 12,
            "url": PULL_URL,
            "title": "Add a lever",
        })
    );
    let crossing = desk.record(result["crossing"].as_str().unwrap()).unwrap();
    assert_eq!(
        (&crossing["action"], &crossing["result"]),
        (&json!("github.pr-create"), &json!(PULL_URL))
    );
}

#[test]
fn pr_create_with_a_private_body_pushes_nothing_and_runs_no_gh_and_a_clean_body_is_sent() {
    let desk = Desk::new();
    desk.refuse(&["pr.*"], "github.*");
    desk.git(&desk.checkout(), &["switch", "-q", "-c", "feature"]);
    desk.commit("Add a lever");
    let fields = refused(desk.call(
        "pr-create",
        &["--title", "Add a lever", "--body", "For acme-123."],
    ));
    let parts: Vec<&Value> = (fields["failures"].as_array().unwrap().iter())
        .map(|failure| &failure["part"])
        .collect();
    assert_eq!(parts, [&json!("pr.body")]);
    assert_eq!(desk.remote_head("feature"), None);
    assert_eq!(desk.gh_calls(), 0);
    desk.gh_answers(&json!(format!("{PULL_URL}\n")), 0, "");
    let result = desk.output(
        "pr-create",
        &["--title", "Add a lever", "--body", "Levers lift."],
    );
    assert_eq!(result["sent"], PULL_URL);
    assert!(!desk.gh_argv().contains(&"--draft".to_owned()));
}

#[test]
fn pr_edit_sends_only_the_fields_given_and_a_private_title_is_refused() {
    let desk = Desk::new();
    desk.refuse(&["pr.title"], "github.*");
    refused(desk.call("pr-edit", &["--number", "12", "--title", "acme-123 levers"]));
    assert_eq!(desk.gh_calls(), 0);
    desk.gh_answers(&json!({"html_url": PULL_URL, "number": 12}), 0, "");
    let result = desk.output("pr-edit", &["--number", "12", "--title", "Levers"]);
    assert_eq!(
        desk.gh_argv(),
        [
            "api",
            "--method",
            "PATCH",
            "repos/octo/widgets/pulls/12",
            "--input",
            "-"
        ]
    );
    let sent: Value = serde_json::from_str(&desk.gh_file("gh-stdin.txt")).unwrap();
    assert_eq!(sent, json!({"title": "Levers"}));
    assert_eq!(result["sent"], PULL_URL);
}

#[test]
fn pr_edit_without_a_title_or_body_is_refused_by_board_and_with_a_body_is_sent() {
    let desk = Desk::new();
    assert_eq!(
        failed(desk.call("pr-edit", &["--number", "12"])),
        (1, json!("board::plugin::bad_input"))
    );
    desk.gh_answers(&json!({"html_url": PULL_URL, "number": 12}), 0, "");
    desk.output("pr-edit", &["--number", "12", "--body", "Levers lift."]);
    let sent: Value = serde_json::from_str(&desk.gh_file("gh-stdin.txt")).unwrap();
    assert_eq!(sent, json!({"body": "Levers lift."}));
}

#[test]
fn a_comment_is_checked_then_posted_and_a_private_one_is_not() {
    let desk = Desk::new();
    desk.refuse(&["comment.body"], "github.*");
    refused(desk.call("comment", &["--number", "7", "--body", "Ask acme-123."]));
    assert_eq!(desk.gh_calls(), 0);
    let url = format!("{ISSUE_URL}#issuecomment-1");
    desk.gh_answers(&json!({"html_url": url, "id": 1}), 0, "");
    let result = desk.output("comment", &["--number", "7", "--body", "Looks good."]);
    assert_eq!(
        desk.gh_argv(),
        [
            "api",
            "--method",
            "POST",
            "repos/octo/widgets/issues/7/comments",
            "--input",
            "-"
        ]
    );
    let sent: Value = serde_json::from_str(&desk.gh_file("gh-stdin.txt")).unwrap();
    assert_eq!(sent, json!({"body": "Looks good."}));
    assert_eq!(result["sent"], url);
}

#[test]
fn issue_create_opens_the_issue_and_writes_its_record_and_a_private_title_is_refused() {
    let desk = Desk::new();
    desk.refuse(&["issue.*"], "github.*");
    refused(desk.call(
        "issue-create",
        &["--title", "acme-123 wobbles", "--body", "It wobbles."],
    ));
    assert_eq!(desk.gh_calls(), 0);
    desk.gh_answers(&json!({"html_url": ISSUE_URL, "number": 7}), 0, "");
    let result = desk.output(
        "issue-create",
        &["--title", "Widgets wobble", "--body", "It wobbles."],
    );
    assert_eq!(
        desk.gh_argv(),
        [
            "api",
            "--method",
            "POST",
            "repos/octo/widgets/issues",
            "--input",
            "-"
        ]
    );
    let sent: Value = serde_json::from_str(&desk.gh_file("gh-stdin.txt")).unwrap();
    assert_eq!(
        sent,
        json!({"title": "Widgets wobble", "body": "It wobbles."})
    );
    assert_eq!(
        (&result["sent"], &result["path"]),
        (&json!(ISSUE_URL), &json!("/gh/octo/widgets/issues/7"))
    );
    let record = desk.record("/gh/octo/widgets/issues/7").unwrap();
    assert_eq!(
        record,
        json!({
            "$type": "github/issue/v1",
            "owner": "octo",
            "repo": "widgets",
            "number": 7,
            "url": ISSUE_URL,
            "title": "Widgets wobble",
        })
    );
}

#[test]
fn a_failing_gh_after_a_pass_is_reported_and_no_record_is_written() {
    let desk = Desk::new();
    desk.gh_answers(&json!(""), 1, "HTTP 422: Validation Failed\n");
    let flags = ["--title", "Widgets wobble", "--body", "It wobbles."];
    let (ran, failure) = desk.call("issue-create", &flags);
    assert_eq!(
        (ran.code, &failure["code"]),
        (1, &json!("github::gh::failed"))
    );
    assert!(
        failure["message"]
            .as_str()
            .unwrap()
            .contains("Validation Failed")
    );
    assert_eq!(desk.record("/gh/octo/widgets/issues/7"), None);
    desk.gh_answers(&json!({"html_url": ISSUE_URL, "number": 7}), 0, "");
    assert_eq!(desk.output("issue-create", &flags)["sent"], ISSUE_URL);
}

#[test]
fn a_push_that_lands_before_gh_fails_is_recorded_and_gh_working_records_the_url() {
    let desk = Desk::new();
    desk.git(&desk.checkout(), &["switch", "-q", "-c", "feature"]);
    let sha = desk.commit("Add a lever");
    desk.gh_answers(&json!(""), 1, "could not create\n");
    let flags = ["--title", "Add a lever", "--body", "Levers lift."];
    let (ran, failure) = desk.call("pr-create", &flags);
    assert_eq!(
        (ran.code, &failure["code"]),
        (1, &json!("github::gh::failed"))
    );
    assert_eq!(desk.remote_head("feature"), Some(sha.clone()));
    let crossings = std::fs::read_dir(desk.personal.workdir().join("crossings")).unwrap();
    let results: Vec<Value> = (crossings.map(|entry| entry.unwrap().path()))
        .map(|path| serde_json::from_str::<Value>(&std::fs::read_to_string(path).unwrap()).unwrap())
        .map(|crossing| crossing["result"].clone())
        .collect();
    assert_eq!(
        results,
        [json!(format!("{sha}:refs/heads/feature [new branch]"))]
    );
    desk.gh_answers(&json!(format!("{PULL_URL}\n")), 0, "");
    let result = desk.output("pr-create", &flags);
    let crossing = desk.record(result["crossing"].as_str().unwrap()).unwrap();
    assert_eq!(crossing["result"], PULL_URL);
}

#[test]
fn a_boundary_naming_a_missing_control_passes_on_the_controls_code_and_with_the_control_passes() {
    let desk = Desk::new();
    desk.refuse(&["comment.body"], "github.*");
    let file = desk.personal.workdir().join("controls/names.json");
    let control = std::fs::read_to_string(&file).unwrap();
    remove(&file);
    let flags = ["--number", "7", "--body", "Fine."];
    let (ran, failure) = desk.call("comment", &flags);
    assert_eq!(
        (ran.code, &failure["code"]),
        (1, &json!("github::boundary::refused"))
    );
    assert_eq!(failure["fields"]["code"], "boundary::board::refused");
    assert_eq!(failure["fields"]["cause"], "rules::rule::not_found");
    assert_eq!(desk.gh_calls(), 0);
    desk.write("controls/names.json", &control);
    let url = format!("{ISSUE_URL}#issuecomment-1");
    desk.gh_answers(&json!({"html_url": url}), 0, "");
    assert_eq!(desk.output("comment", &flags)["sent"], url);
}

#[test]
fn a_record_that_fails_after_the_pull_request_is_opened_says_it_was_sent_and_without_it_the_record_is_written()
 {
    let desk = Desk::new();
    desk.git(&desk.checkout(), &["switch", "-q", "-c", "feature"]);
    desk.commit("Add a lever");
    desk.gh_answers(&json!(format!("{PULL_URL}\n")), 0, "");
    let taken = "gh/octo/widgets/pulls/12.json";
    desk.write(taken, "{}");
    let flags = ["--title", "Add a lever", "--body", "Levers lift."];
    let (ran, failure) = desk.call("pr-create", &flags);
    assert_eq!(ran.code, 1, "{failure}");
    assert_eq!(failure["code"], "github::board::refused");
    assert_eq!(failure["fields"]["sent"], PULL_URL);
    let message = failure["message"].as_str().unwrap();
    assert!(
        message.starts_with(&format!(
            "{PULL_URL} was already sent, so do not send it again; "
        )),
        "{message}"
    );
    remove(&desk.personal.workdir().join(taken));
    assert_eq!(desk.output("pr-create", &flags)["sent"], PULL_URL);
}

#[test]
fn a_crossing_that_cannot_be_completed_after_a_comment_is_posted_says_it_was_sent_and_one_that_can_is_completed()
 {
    let desk = Desk::new();
    let url = format!("{ISSUE_URL}#issuecomment-1");
    desk.gh_answers(&json!({"html_url": url, "id": 1}), 0, "");
    let crossings = desk.personal.workdir().join("crossings");
    desk.gh_also(&format!("rm -rf '{}'", crossings.display()));
    let flags = ["--number", "7", "--body", "Looks good."];
    let (ran, failure) = desk.call("comment", &flags);
    assert_eq!(ran.code, 1, "{failure}");
    assert_eq!(failure["fields"]["sent"], url);
    let message = failure["message"].as_str().unwrap();
    assert!(
        message.contains(&format!("{url} was already sent")),
        "{message}"
    );
    desk.gh_also("");
    assert_eq!(desk.output("comment", &flags)["sent"], url);
}

#[test]
fn pr_create_failing_after_the_push_says_the_pull_request_was_not_opened_and_running_it_again_opens_it()
 {
    let desk = Desk::new();
    desk.git(&desk.checkout(), &["switch", "-q", "-c", "feature"]);
    let sha = desk.commit("Add a lever");
    // A pre-push hook that removes the crossings, so the push goes but its
    // result cannot be recorded.
    let hook = desk.checkout().join(".git/hooks/pre-push");
    let crossings = desk.personal.workdir().join("crossings");
    put_file(
        &hook,
        &format!("#!/bin/sh\nrm -rf '{}'\n", crossings.display()),
    );
    executable(&hook);
    desk.gh_answers(&json!(format!("{PULL_URL}\n")), 0, "");
    let flags = ["--title", "Add a lever", "--body", "Levers lift."];
    let (ran, failure) = desk.call("pr-create", &flags);
    assert_eq!(ran.code, 1, "{failure}");
    assert_eq!(desk.remote_head("feature"), Some(sha));
    assert_eq!(desk.gh_calls(), 0);
    let message = failure["message"].as_str().unwrap();
    assert!(
        message.contains("the pull request was not opened"),
        "{message}"
    );
    assert!(
        message.contains("run pr-create again to open it"),
        "{message}"
    );
    assert!(!message.contains("do not send it again"), "{message}");
    remove(&hook);
    assert_eq!(desk.output("pr-create", &flags)["sent"], PULL_URL);
}
