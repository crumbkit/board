//! `merge`: gh squash-merges only after the change's land gate and the
//! boundary pass, and the change then moves to done once GitHub says the
//! pull request merged.

use super::desk::{Desk, record_in};
use super::*;

const PULL_URL: &str = "https://github.com/octo/widgets/pull/12";
const HEAD: &str = "0123456789abcdef0123456789abcdef01234567";
const CHANGE: &str = "/changes/lever";
const TESTED: &str = "# Lever\n\n## What the tests prove\n\nIt lands.\n\n## What the tests do not prove\n\nRetries.\n";
/// A plan that also has the section the team board's flow asks for.
const TEAM_TESTED: &str = "# Lever\n\n## What the tests prove\n\nIt lands.\n\n## What the tests do not prove\n\nRetries.\n\n## Team tests\n\nThe team ran it.\n";
/// The version of the development flow that only the team board holds.
const TEAM_FLOW: &str = "/flows/development/7ea3000000000000";

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
    lever(&desk.personal, &record, plan);
}

/// Writes `record` at /changes/lever in `board`, with a plan record whose
/// text holds `plan` and pull request 12.
fn lever(board: &Scenario, record: &Value, plan: &str) {
    let write = |path: &str, text: &str| {
        board.write(path, Item::File(text));
    };
    write("changes/lever/plan.md", plan);
    let planned = json!({
        "$type": "changes/plan/v1",
        "text": "/changes/lever/plan.md",
        "uses": [],
        "records": [],
        "answers": {"ux": "none", "data": "none", "lifecycle": "none"},
    });
    write("changes/lever/plans/1.json", &planned.to_string());
    let pull = json!({
        "$type": "github/pull/v1",
        "owner": "octo",
        "repo": "widgets",
        "number": 12,
        "url": PULL_URL,
    });
    write("gh/octo/widgets/pulls/12.json", &pull.to_string());
    write("changes/lever.json", &record.to_string());
}

/// Writes, in the team board, a `changes/change/v2` at /changes/lever in
/// land at [`HEAD`] whose plan's text holds `plan`, and the [`TEAM_FLOW`]
/// version it follows: the development flow whose land phase also asks for
/// a Team tests section under the check `team-tests`, which no shipped
/// flow has. Then commits the team board and moves the pin.
fn team_change(desk: &Desk, team: &Scenario, plan: &str) {
    let mut flow: Value = serde_json::from_str(include_str!(
        "../../../../../board-plugin-changes/flows/development.json"
    ))
    .unwrap();
    let phases = flow["phases"].as_array_mut().unwrap();
    let land = phases.iter_mut().find(|phase| phase["name"] == "land");
    land.unwrap()["checks"].as_array_mut().unwrap().push(json!({
        "id": "team-tests", "kind": "sections", "sections": ["Team tests"],
    }));
    let version = format!("{}.json", TEAM_FLOW.trim_start_matches('/'));
    team.write(&version, Item::File(&flow.to_string()));
    let record = json!({
        "$type": "changes/change/v2",
        "title": "Add a lever",
        "mount": "/repos/widgets",
        "phase": "land",
        "touched": "2026-11-10",
        "head": HEAD,
        "flow": TEAM_FLOW,
        "plan": "/changes/lever/plans/1",
        "pulls": ["/gh/octo/widgets/pulls/12"],
    });
    lever(team, &record, plan);
    desk.pin(team);
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

#[test]
fn merge_run_in_a_board_the_personal_board_mounts_lands_that_boards_change() {
    let desk = Desk::new();
    let team = desk.team();
    team_change(&desk, &team, TEAM_TESTED);
    pulls(&desk, true);
    desk.gh_answers(&json!(""), 0, "");
    let at = team.path();
    let (ran, document) = desk.call_in(
        &["-C", &at],
        "merge",
        &merging("Add a lever", "Levers lift.", &["--change", CHANGE]),
    );
    assert_eq!(ran.code, 0, "{document}");
    let result = &document["data"]["output"];
    assert_eq!(result["phase"], "done");
    assert_eq!(merges(&desk), 1);
    let argv = desk.gh_file("gh-merge-argv.txt");
    assert!(argv.lines().any(|line| line == HEAD), "{argv}");
    assert_eq!(record_in(&team, CHANGE).unwrap()["phase"], "done");
    let crossing = desk.record(result["crossing"].as_str().unwrap()).unwrap();
    assert_eq!(
        (&crossing["action"], &crossing["result"]),
        (&json!("github.merge"), &json!(PULL_URL))
    );
}

#[test]
fn merge_in_a_mounted_board_refuses_exactly_what_changes_next_there_misses_and_merges_once_it_passes()
 {
    let desk = Desk::new();
    let team = desk.team();
    team_change(&desk, &team, "# Lever\n");
    pulls(&desk, true);
    let at = team.path();
    let missing = || -> Vec<Value> {
        let next = ["-C", &at, "--json", "call", "changes.next"];
        let arguments: Vec<&str> = next.into_iter().chain(["--path", CHANGE]).collect();
        let ran = desk.run_in(&desk.checkout(), &arguments, &[]);
        let document = json(&ran);
        assert_eq!(ran.code, 0, "{document}");
        let gate = document["data"]["output"]["gate"].as_array().unwrap();
        (gate.iter())
            .filter(|check| check["status"] == "missing" && check["blocks"] == true)
            .map(|check| check["check"].clone())
            .collect()
    };
    let flags = merging("Add a lever", "Levers lift.", &["--change", CHANGE]);
    let (ran, document) = desk.call_in(&["-C", &at], "merge", &flags);
    assert_eq!(ran.code, 1, "{document}");
    assert_eq!(document["code"], "github::merge::gate");
    assert_eq!(document["fields"]["missing"], json!(missing()));
    assert_eq!(missing(), [json!("test-sections"), json!("team-tests")]);
    assert_eq!(desk.gh_calls(), 0);
    team.write("changes/lever/plan.md", Item::File(TEAM_TESTED));
    desk.pin(&team);
    assert_eq!(missing(), Vec::<Value>::new());
    desk.gh_answers(&json!(""), 0, "");
    let (ran, document) = desk.call_in(&["-C", &at], "merge", &flags);
    assert_eq!(ran.code, 0, "{document}");
    assert_eq!(merges(&desk), 1);
}

#[test]
fn merge_naming_a_change_under_a_mount_is_refused_and_the_same_change_named_in_its_own_board_merges()
 {
    let desk = Desk::new();
    let team = desk.team();
    team_change(&desk, &team, TEAM_TESTED);
    pulls(&desk, true);
    let mounted = "/repos/team/changes/lever";
    let (ran, document) = desk.call_in(
        &["--personal"],
        "merge",
        &merging("Add a lever", "Levers lift.", &["--change", mounted]),
    );
    assert_eq!(ran.code, 1, "{document}");
    assert_eq!(document["code"], "github::merge::change", "{document}");
    assert_eq!(desk.gh_calls(), 0);
    desk.gh_answers(&json!(""), 0, "");
    let at = team.path();
    let (ran, document) = desk.call_in(
        &["-C", &at],
        "merge",
        &merging("Add a lever", "Levers lift.", &["--change", CHANGE]),
    );
    assert_eq!(ran.code, 0, "{document}");
    assert_eq!(merges(&desk), 1);
}

#[test]
fn a_change_in_the_personal_board_still_merges_beside_a_mounted_board_that_holds_one_of_the_same_name()
 {
    let desk = Desk::new();
    let team = desk.team();
    team_change(&desk, &team, TEAM_TESTED);
    change(&desk, "land", HEAD, TESTED);
    pulls(&desk, true);
    desk.gh_answers(&json!(""), 0, "");
    let (ran, document) = desk.call_in(
        &["--personal"],
        "merge",
        &merging("Add a lever", "Levers lift.", &["--change", CHANGE]),
    );
    assert_eq!(ran.code, 0, "{document}");
    assert_eq!(desk.record(CHANGE).unwrap()["phase"], "done");
    assert_eq!(record_in(&team, CHANGE).unwrap()["phase"], "land");
}

#[test]
fn a_change_the_commands_board_does_not_hold_is_refused_with_both_forms_and_merges_with_personal() {
    let desk = Desk::new();
    let team = desk.team();
    change(&desk, "land", HEAD, TESTED);
    pulls(&desk, true);
    let flags = merging("Add a lever", "Levers lift.", &["--change", CHANGE]);
    let at = team.path();
    let (ran, document) = desk.call_in(&["-C", &at], "merge", &flags);
    assert_eq!(ran.code, 1, "{document}");
    assert_eq!(
        (&document["code"], &document["fields"]["code"]),
        (
            &json!("github::board::refused"),
            &json!("changes::change::not_found")
        ),
        "{document}"
    );
    let hint = document["recovery"]["text"].as_str().unwrap_or_default();
    assert!(
        hint.contains("--personal") && hint.contains("-C"),
        "{document}"
    );
    assert_eq!(desk.gh_calls(), 0);
    desk.gh_answers(&json!(""), 0, "");
    let (ran, document) = desk.call_in(&["--personal"], "merge", &flags);
    assert_eq!(ran.code, 0, "{document}");
    assert_eq!(desk.record(CHANGE).unwrap()["phase"], "done");
}
