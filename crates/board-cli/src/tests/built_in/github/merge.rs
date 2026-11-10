//! `merge`: gh squash-merges only after the change's land gate and the
//! boundary pass, and the change then moves to done once GitHub says the
//! pull request merged.

use super::desk::Desk;
use super::*;

const PULL_URL: &str = "https://github.com/octo/widgets/pull/12";
const HEAD: &str = "0123456789abcdef0123456789abcdef01234567";
const CHANGE: &str = "/changes/lever";
const TESTED: &str = "# Lever\n\n## What the tests prove\n\nIt lands.\n\n## What the tests do not prove\n\nRetries.\n";

/// Enables the changes plugin and writes a change in `phase` at `head`,
/// with a plan record whose text holds `plan` and pull request 12. An
/// empty `head` leaves the change without one.
fn change(desk: &Desk, phase: &str, head: &str, plan: &str) {
    let enabled = desk.run_in(
        desk.personal.workdir(),
        &["plugin", "enable", "changes"],
        &[],
    );
    assert_eq!(enabled.code, 0, "{}", text(&enabled.stderr));
    desk.write("changes/lever/plan.md", plan);
    let planned = json!({
        "$type": "changes/plan/v1",
        "text": "/changes/lever/plan.md",
        "uses": [],
        "records": [],
    });
    desk.write("changes/lever/plans/1.json", &planned.to_string());
    let pull = json!({
        "$type": "github/pull/v1",
        "owner": "octo",
        "repo": "widgets",
        "number": 12,
        "url": PULL_URL,
    });
    desk.write("gh/octo/widgets/pulls/12.json", &pull.to_string());
    let mut record = json!({
        "$type": "changes/change/v1",
        "title": "Add a lever",
        "mount": "/repos/widgets",
        "phase": phase,
        "touched": "2026-11-10",
        "head": head,
        "plan": "/changes/lever/plans/1",
        "pulls": ["/gh/octo/widgets/pulls/12"],
    });
    if head.is_empty() {
        record.as_object_mut().unwrap().remove("head");
    }
    desk.write("changes/lever.json", &record.to_string());
}

/// Makes the stub gh answer `gh api` with a pull request whose `merged` is
/// `merged`, and keep each other run's arguments in `gh-merge-argv.txt`,
/// its stdin in `gh-merge-stdin.txt` and a line in `gh-merges.txt`.
fn pulls(desk: &Desk, merged: bool) {
    desk.gh_also(&format!(
        "if [ \"$1\" = api ]; then printf '%s' '{{\"merged\":{merged}}}'; exit 0; fi\n\
         cp \"$here/gh-argv.txt\" \"$here/gh-merge-argv.txt\"\n\
         cp \"$here/gh-stdin.txt\" \"$here/gh-merge-stdin.txt\"\n\
         echo merge >> \"$here/gh-merges.txt\""
    ));
}

/// How many times the stub gh ran other than for `gh api`, once it has.
fn merges(desk: &Desk) -> usize {
    desk.gh_file("gh-merges.txt").lines().count()
}

/// The flags of a merge of pull request 12 with `subject` and `body`, and
/// `extra`.
fn merging<'a>(subject: &'a str, body: &'a str, extra: &[&'a str]) -> Vec<&'a str> {
    let mut flags = vec!["--number", "12", "--subject", subject, "--body", body];
    flags.extend_from_slice(extra);
    flags
}

/// The code of a call that failed.
fn failed((ran, document): (Output, Value)) -> (u8, Value) {
    (ran.code, document["code"].clone())
}

#[test]
fn merge_of_a_change_in_land_squash_merges_its_head_and_moves_the_change_to_done() {
    let desk = Desk::new();
    change(&desk, "land", HEAD, TESTED);
    pulls(&desk, true);
    desk.gh_answers(&json!(""), 0, "");
    let result = desk.output(
        "merge",
        &merging("Add a lever (#12)", "Levers lift.", &["--change", CHANGE]),
    );
    let argv: Vec<String> = (desk.gh_file("gh-merge-argv.txt").lines())
        .map(str::to_owned)
        .collect();
    assert_eq!(
        argv,
        [
            "pr",
            "merge",
            "12",
            "--repo",
            "octo/widgets",
            "--squash",
            "--match-head-commit",
            HEAD,
            "--subject",
            "Add a lever (#12)",
            "--body-file",
            "-"
        ]
    );
    assert_eq!(desk.gh_argv(), ["api", "repos/octo/widgets/pulls/12"]);
    assert_eq!(desk.gh_file("gh-merge-stdin.txt"), "Levers lift.");
    assert_eq!(
        (
            &result["sent"],
            &result["head"],
            &result["phase"],
            &result["queued"]
        ),
        (
            &json!(PULL_URL),
            &json!(HEAD),
            &json!("done"),
            &json!(false)
        )
    );
    assert_eq!(desk.record(CHANGE).unwrap()["phase"], "done");
    let crossing = desk.record(result["crossing"].as_str().unwrap()).unwrap();
    assert_eq!(
        (&crossing["action"], &crossing["result"]),
        (&json!("github.merge"), &json!(PULL_URL))
    );
}

#[test]
fn a_merge_github_only_queued_records_the_crossing_and_leaves_the_change_in_land() {
    let desk = Desk::new();
    change(&desk, "land", HEAD, TESTED);
    pulls(&desk, false);
    desk.gh_answers(&json!(""), 0, "");
    let result = desk.output(
        "merge",
        &merging("Add a lever", "Levers lift.", &["--change", CHANGE]),
    );
    assert_eq!(
        (&result["sent"], &result["queued"], result.get("phase")),
        (&json!(PULL_URL), &json!(true), None)
    );
    assert_eq!(desk.record(CHANGE).unwrap()["phase"], "land");
    let crossing = desk.record(result["crossing"].as_str().unwrap()).unwrap();
    assert_eq!(crossing["result"], PULL_URL);
}

#[test]
fn merge_of_a_change_whose_land_gate_misses_a_check_runs_no_gh_and_one_that_passes_merges() {
    let desk = Desk::new();
    change(&desk, "land", HEAD, "# Lever\n");
    pulls(&desk, true);
    let (ran, document) = desk.call(
        "merge",
        &merging("Add a lever", "Levers lift.", &["--change", CHANGE]),
    );
    assert_eq!(ran.code, 1, "{document}");
    assert_eq!(document["code"], "github::merge::gate");
    assert_eq!(document["fields"]["missing"], json!(["test-sections"]));
    assert_eq!(desk.gh_calls(), 0);
    desk.write("changes/lever/plan.md", TESTED);
    desk.gh_answers(&json!(""), 0, "");
    desk.output(
        "merge",
        &merging("Add a lever", "Levers lift.", &["--change", CHANGE]),
    );
    assert_eq!(merges(&desk), 1);
}

#[test]
fn merge_of_a_change_before_land_is_refused_and_runs_no_gh() {
    let desk = Desk::new();
    change(&desk, "review", HEAD, TESTED);
    let (ran, document) = desk.call(
        "merge",
        &merging("Add a lever", "Levers lift.", &["--change", CHANGE]),
    );
    assert_eq!(ran.code, 1, "{document}");
    assert_eq!(
        (&document["code"], &document["fields"]["phase"]),
        (&json!("github::merge::gate"), &json!("review"))
    );
    assert_eq!(desk.gh_calls(), 0);
}

#[test]
fn merge_with_a_private_subject_or_body_is_refused_and_a_clean_one_is_sent() {
    let desk = Desk::new();
    desk.refuse(&["pr.*"], "github.*");
    for (subject, body, part) in [
        ("Fix acme-123", "Levers lift.", "pr.merge.subject"),
        ("Add a lever", "For acme-123.", "pr.merge.body"),
    ] {
        let (ran, document) = desk.call("merge", &merging(subject, body, &["--head", HEAD]));
        assert_eq!(ran.code, 1, "{document}");
        assert_eq!(document["code"], "github::boundary::refused");
        let parts: Vec<&Value> = (document["fields"]["failures"].as_array().unwrap().iter())
            .map(|failure| &failure["part"])
            .collect();
        assert_eq!(parts, [&json!(part)]);
    }
    assert_eq!(desk.gh_calls(), 0);
    pulls(&desk, true);
    desk.gh_answers(&json!(""), 0, "");
    let result = desk.output(
        "merge",
        &merging("Add a lever", "Levers lift.", &["--head", HEAD]),
    );
    assert_eq!(
        (&result["sent"], result.get("phase")),
        (&json!(PULL_URL), None)
    );
}

#[test]
fn merge_names_the_head_it_matches_and_refuses_a_head_the_change_does_not_record() {
    let desk = Desk::new();
    assert_eq!(
        failed(desk.call("merge", &merging("Add a lever", "Levers lift.", &[]))),
        (1, json!("github::merge::head"))
    );
    change(&desk, "land", HEAD, TESTED);
    let other = "fedcba9876543210fedcba9876543210fedcba98";
    assert_eq!(
        failed(desk.call(
            "merge",
            &merging(
                "Add a lever",
                "Levers lift.",
                &["--change", CHANGE, "--head", other]
            ),
        )),
        (1, json!("github::merge::head"))
    );
    assert_eq!(desk.gh_calls(), 0);
}

#[test]
fn merge_of_a_change_with_no_head_or_a_short_one_is_refused_and_runs_no_gh() {
    let desk = Desk::new();
    let flags = merging("Add a lever", "Levers lift.", &["--change", CHANGE]);
    for head in ["", "0123456"] {
        change(&desk, "land", head, TESTED);
        assert_eq!(
            failed(desk.call("merge", &flags)),
            (1, json!("github::merge::head")),
            "{head}"
        );
    }
    assert_eq!(desk.gh_calls(), 0);
    let (ran, document) = desk.call(
        "merge",
        &merging("Add a lever", "Levers lift.", &["--head", "0123456"]),
    );
    assert_eq!(
        (ran.code, &document["code"]),
        (1, &json!("board::plugin::bad_input"))
    );
}

#[test]
fn a_failed_gh_merge_leaves_the_change_in_land_and_a_retry_merges_it() {
    let desk = Desk::new();
    change(&desk, "land", HEAD, TESTED);
    pulls(&desk, true);
    desk.gh_answers(&json!(""), 1, "Head branch was modified\n");
    let flags = merging("Add a lever", "Levers lift.", &["--change", CHANGE]);
    assert_eq!(
        failed(desk.call("merge", &flags)),
        (1, json!("github::gh::failed"))
    );
    assert_eq!(desk.record(CHANGE).unwrap()["phase"], "land");
    desk.gh_answers(&json!(""), 0, "");
    assert_eq!(desk.output("merge", &flags)["phase"], "done");
}
