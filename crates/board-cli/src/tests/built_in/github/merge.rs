//! `merge`: gh squash-merges only after the change's land gate and the
//! boundary pass, and the change then moves to done once GitHub says the
//! pull request merged.

use super::desk::{Desk, record_in};
use super::*;

const PULL_URL: &str = "https://github.com/octo/widgets/pull/12";
pub(super) const HEAD: &str = "0123456789abcdef0123456789abcdef01234567";
pub(super) const CHANGE: &str = "/changes/lever";
pub(super) const TESTED: &str = "# Lever\n\n## What the tests prove\n\nIt lands.\n\n## What the tests do not prove\n\nRetries.\n";
/// A plan that also has the section the team board's flow asks for.
pub(super) const TEAM_TESTED: &str = "# Lever\n\n## What the tests prove\n\nIt lands.\n\n## What the tests do not prove\n\nRetries.\n\n## Team tests\n\nThe team ran it.\n";
/// The version of the development flow that only the team board holds.
const TEAM_FLOW: &str = "/flows/development/7ea3000000000000";

/// Enables the changes plugin and writes a change in `phase` at `head`,
/// with a plan record whose text holds `plan` and pull request 12. An
/// empty `head` leaves the change without one.
pub(super) fn change(desk: &Desk, phase: &str, head: &str, plan: &str) {
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
pub(super) fn team_change(desk: &Desk, team: &Scenario, plan: &str) {
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
pub(super) fn pulls(desk: &Desk, merged: bool) {
    desk.gh_also(&format!(
        "if [ \"$1\" = api ]; then printf '%s' '{{\"merged\":{merged},\"title\":\"Add a lever\",\"body\":\"Levers lift.\"}}'; exit 0; fi\n\
         cp \"$here/gh-argv.txt\" \"$here/gh-merge-argv.txt\"\n\
         cp \"$here/gh-stdin.txt\" \"$here/gh-merge-stdin.txt\"\n\
         echo merge >> \"$here/gh-merges.txt\""
    ));
}

/// How many times the stub gh ran other than for `gh api`.
fn merges(desk: &Desk) -> usize {
    std::fs::read_to_string(desk.bin().join("gh-merges.txt"))
        .map_or(0, |merges| merges.lines().count())
}

/// The flags of a merge of pull request 12 with `subject` and `body`, and
/// `extra`.
pub(super) fn merging<'a>(subject: &'a str, body: &'a str, extra: &[&'a str]) -> Vec<&'a str> {
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

/// A change in land whose plan closes the attached open task
/// /tasks/left.md at merge, with the tasks plugin enabled.
fn closing_at_merge(desk: &Desk) {
    let plan = format!("{TESTED}\n## Closes at merge\n\n- `/tasks/left.md`\n");
    change(desk, "land", HEAD, &plan);
    let enabled = desk.run_in(desk.personal.workdir(), &["plugin", "enable", "tasks"], &[]);
    assert_eq!(enabled.code, 0, "{}", text(&enabled.stderr));
    desk.write(
        "tasks/left.md",
        "---\n$type: tasks/task/v2\ntitle: Left\nstatus: open\n---\n",
    );
    let mut planned = desk.record("/changes/lever/plans/1").unwrap();
    planned["records"] = json!(["/tasks/left.md"]);
    desk.write("changes/lever/plans/1.json", &planned.to_string());
}

#[test]
fn merge_closes_the_task_the_plan_closes_at_merge_before_it_moves_the_change_to_done() {
    let desk = Desk::new();
    closing_at_merge(&desk);
    pulls(&desk, true);
    desk.gh_answers(&json!(""), 0, "");
    let result = desk.output(
        "merge",
        &merging("Add a lever (#12)", "Levers lift.", &["--change", CHANGE]),
    );
    assert_eq!(result["phase"], "done", "{result}");
    assert_eq!(desk.record(CHANGE).unwrap()["phase"], "done");
    let left = std::fs::read_to_string(desk.personal.workdir().join("tasks/left.md")).unwrap();
    assert!(left.contains(r#"status: "done""#), "{left}");
}

#[test]
fn merge_leaves_the_change_in_land_when_the_plan_no_longer_closes_an_open_task_at_merge() {
    let desk = Desk::new();
    closing_at_merge(&desk);
    desk.write("changes/lever/plan.md", TESTED);
    pulls(&desk, true);
    desk.gh_answers(&json!(""), 0, "");
    let (ran, document) = desk.call(
        "merge",
        &merging("Add a lever (#12)", "Levers lift.", &["--change", CHANGE]),
    );
    assert_eq!(ran.code, 1, "{document}");
    assert_eq!(document["code"], "github::merge::advance", "{document}");
    let message = document["message"].as_str().unwrap();
    assert!(message.contains("/tasks/left.md"), "{message}");
    assert_eq!(desk.record(CHANGE).unwrap()["phase"], "land");
    let left = std::fs::read_to_string(desk.personal.workdir().join("tasks/left.md")).unwrap();
    assert!(left.contains("status: open"), "{left}");
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
fn a_merge_run_with_checkout_moves_its_change_to_done_as_one_run_without_it_does() {
    let desk = Desk::new();
    change(&desk, "land", HEAD, TESTED);
    pulls(&desk, true);
    desk.gh_answers(&json!(""), 0, "");
    let flags = merging("Add a lever", "Levers lift.", &["--change", CHANGE]);
    let (ran, document) = desk.call_in(&["--checkout"], "merge", &flags);
    assert_eq!(ran.code, 0, "{document}");
    assert_eq!(document["data"]["output"]["phase"], "done");
    assert_eq!(merges(&desk), 1);
    assert_eq!(desk.record(CHANGE).unwrap()["phase"], "done");
}

#[test]
fn a_change_the_personal_board_does_not_hold_is_refused_there_with_a_hint_that_does_not_name_personal()
 {
    let desk = Desk::new();
    change(&desk, "land", HEAD, TESTED);
    let typo = ["--change", "/changes/levr"];
    let flags = merging("Add a lever", "Levers lift.", &typo);
    let (ran, document) = desk.call_in(&["--personal"], "merge", &flags);
    assert_eq!(ran.code, 1, "{document}");
    assert_eq!(document["fields"]["code"], "changes::change::not_found");
    let hint = document["recovery"]["text"].as_str().unwrap_or_default();
    assert!(!hint.contains("--personal"), "{document}");
    assert!(hint.contains("board changes open"), "{document}");
    assert_eq!(desk.gh_calls(), 0);
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

/// Rewrites the change's `pulls` to `pulls`.
fn pulling(desk: &Desk, pulls: &[&str]) {
    let mut record = desk.record(CHANGE).unwrap();
    record["pulls"] = json!(pulls);
    desk.write("changes/lever.json", &record.to_string());
}

#[test]
fn merge_with_only_a_change_takes_subject_body_and_number_from_its_pull_request() {
    let desk = Desk::new();
    change(&desk, "land", HEAD, TESTED);
    pulls(&desk, true);
    desk.gh_answers(&json!(""), 0, "");
    let result = desk.output("merge", &["--change", CHANGE]);
    assert_eq!(result["phase"], "done");
    let argv = desk.gh_file("gh-merge-argv.txt");
    let argv: Vec<&str> = argv.lines().collect();
    assert_eq!(argv[2], "12");
    assert_eq!(
        argv[argv.iter().position(|a| *a == "--subject").unwrap() + 1],
        "Add a lever (#12)"
    );
    assert_eq!(desk.gh_file("gh-merge-stdin.txt"), "Levers lift.");
}

#[test]
fn a_given_subject_and_body_win_over_the_pull_request() {
    let desk = Desk::new();
    change(&desk, "land", HEAD, TESTED);
    pulls(&desk, true);
    desk.gh_answers(&json!(""), 0, "");
    desk.output(
        "merge",
        &[
            "--change",
            CHANGE,
            "--subject",
            "Mine",
            "--body",
            "Own words.",
        ],
    );
    let argv = desk.gh_file("gh-merge-argv.txt");
    assert!(argv.lines().any(|line| line == "Mine"), "{argv}");
    assert_eq!(desk.gh_file("gh-merge-stdin.txt"), "Own words.");
}

/// The code, message and `fields.pulls` of a refused merge.
fn refusal(desk: &Desk, flags: &[&str]) -> (Value, String, Value) {
    let (ran, document) = desk.call("merge", flags);
    assert_eq!(ran.code, 1, "{document}");
    let message = document["message"].as_str().unwrap_or_default().to_owned();
    (
        document["code"].clone(),
        message,
        document["fields"]["pulls"].clone(),
    )
}

#[test]
fn a_change_with_no_pull_or_two_pulls_in_the_repository_needs_number_and_with_it_merges() {
    let other = "/gh/octo/other/pulls/3";
    let two = ["/gh/octo/widgets/pulls/12", "/gh/octo/widgets/pulls/13"];
    let number = || json!("github::merge::number");
    for (recorded, message, numbers) in [
        (
            two.to_vec(),
            "the change records several pull requests of octo/widgets, #12, #13; pass --number \
             <n> for the one to merge",
            json!([12, 13]),
        ),
        (
            vec![other],
            "the change records no readable github/pull/v1 record of octo/widgets; pass \
             --number <n>",
            json!([]),
        ),
    ] {
        let desk = Desk::new();
        change(&desk, "land", HEAD, TESTED);
        let pull = json!({
            "$type": "github/pull/v1", "owner": "octo", "repo": "widgets", "number": 13,
            "url": "https://github.com/octo/widgets/pull/13",
        });
        desk.write("gh/octo/widgets/pulls/13.json", &pull.to_string());
        let pull = json!({
            "$type": "github/pull/v1", "owner": "octo", "repo": "other", "number": 3,
            "url": "https://github.com/octo/other/pull/3",
        });
        desk.write("gh/octo/other/pulls/3.json", &pull.to_string());
        pulling(&desk, &recorded);
        pulls(&desk, true);
        desk.gh_answers(&json!(""), 0, "");
        assert_eq!(
            refusal(&desk, &["--change", CHANGE]),
            (number(), message.to_owned(), numbers),
            "{recorded:?}"
        );
        assert_eq!(desk.gh_calls(), 0);
        let number = if recorded.len() == 2 { "13" } else { "12" };
        let result = desk.output("merge", &["--change", CHANGE, "--number", number]);
        assert_eq!(result["phase"], "done");
    }
    // A pull of another repository does not count, and one of this does.
    let desk = Desk::new();
    change(&desk, "land", HEAD, TESTED);
    pulling(&desk, &[other, two[0]]);
    pulls(&desk, true);
    desk.gh_answers(&json!(""), 0, "");
    assert_eq!(desk.output("merge", &["--change", CHANGE])["phase"], "done");
    // Without a change, the number is required too.
    assert_eq!(
        refusal(&desk, &["--head", HEAD]),
        (
            number(),
            "merge needs the pull request to merge; pass --change /changes/<name> or --number <n>"
                .to_owned(),
            json!([])
        )
    );
}

#[test]
fn a_pull_the_change_records_through_a_mount_gives_the_number_and_one_of_another_repository_does_not()
 {
    let desk = Desk::new();
    let team = desk.team();
    change(&desk, "land", HEAD, TESTED);
    for (repo, number) in [("widgets", 12), ("other", 3)] {
        let pull = json!({
            "$type": "github/pull/v1", "owner": "octo", "repo": repo, "number": number,
            "url": format!("https://github.com/octo/{repo}/pull/{number}"),
        });
        let path = format!("gh/octo/{repo}/pulls/{number}.json");
        team.write(&path, Item::File(&pull.to_string()));
    }
    desk.pin(&team);
    pulls(&desk, true);
    desk.gh_answers(&json!(""), 0, "");
    pulling(&desk, &["/repos/team/gh/octo/other/pulls/3"]);
    let (ran, document) = desk.call_in(&["--personal"], "merge", &["--change", CHANGE]);
    assert_eq!(ran.code, 1, "{document}");
    assert_eq!(
        (&document["code"], &document["fields"]["pulls"]),
        (&json!("github::merge::number"), &json!([]))
    );
    assert_eq!(desk.gh_calls(), 0);
    pulling(&desk, &["/repos/team/gh/octo/widgets/pulls/12"]);
    let (ran, document) = desk.call_in(&["--personal"], "merge", &["--change", CHANGE]);
    assert_eq!(ran.code, 0, "{document}");
    assert_eq!(document["data"]["output"]["phase"], "done", "{document}");
    let argv = desk.gh_file("gh-merge-argv.txt");
    assert_eq!(argv.lines().nth(2), Some("12"), "{argv}");
}

#[test]
fn a_gate_refusal_hint_names_each_missing_checks_remedy() {
    let desk = Desk::new();
    let team = desk.team();
    team_change(&desk, &team, "# Lever\n");
    pulls(&desk, true);
    let at = team.path();
    let next = [
        "-C",
        &at,
        "--json",
        "call",
        "changes.next",
        "--path",
        CHANGE,
    ];
    let ran = desk.run_in(&desk.checkout(), &next, &[]);
    let gate = json(&ran)["data"]["output"]["gate"].clone();
    let remedies: Vec<String> = (gate.as_array().unwrap().iter())
        .filter(|check| check["status"] == "missing" && check["blocks"] == true)
        .map(|check| {
            format!(
                "{}: {}",
                check["check"].as_str().unwrap(),
                check["remedy"].as_str().unwrap()
            )
        })
        .collect();
    assert_eq!(remedies.len(), 2, "{gate}");
    let (ran, document) = desk.call_in(&["-C", &at], "merge", &["--change", CHANGE]);
    assert_eq!(ran.code, 1, "{document}");
    assert_eq!(document["code"], "github::merge::gate");
    assert_eq!(document["recovery"]["text"], json!(remedies.join("; ")));
}

#[test]
fn a_defaulted_subject_or_body_that_fails_the_boundary_is_refused_and_a_clean_one_merges() {
    let desk = Desk::new();
    change(&desk, "land", HEAD, TESTED);
    desk.refuse(&["pr.*"], "github.*");
    for (title, body, part) in [
        ("Fix acme-123", "Levers lift.", "pr.merge.subject"),
        ("Add a lever", "For acme-123.", "pr.merge.body"),
    ] {
        desk.gh_also(&format!(
            "if [ \"$1\" = api ]; then printf '%s' '{{\"merged\":true,\"title\":\"{title}\",\"body\":\"{body}\"}}'; exit 0; fi\n\
             echo merge >> \"$here/gh-merges.txt\""
        ));
        let (ran, document) = desk.call("merge", &["--change", CHANGE]);
        assert_eq!(ran.code, 1, "{document}");
        assert_eq!(document["code"], "github::boundary::refused");
        let parts: Vec<&Value> = (document["fields"]["failures"].as_array().unwrap().iter())
            .map(|failure| &failure["part"])
            .collect();
        assert_eq!(parts, [&json!(part)]);
    }
    assert_eq!(merges(&desk), 0);
    pulls(&desk, true);
    desk.gh_answers(&json!(""), 0, "");
    assert_eq!(desk.output("merge", &["--change", CHANGE])["phase"], "done");
}

#[test]
fn a_pull_request_with_a_null_body_merges_with_an_empty_body() {
    let desk = Desk::new();
    change(&desk, "land", HEAD, TESTED);
    desk.gh_also(
        "if [ \"$1\" = api ]; then printf '%s' '{\"merged\":true,\"title\":\"Add a lever\",\"body\":null}'; exit 0; fi\n\
         cp \"$here/gh-argv.txt\" \"$here/gh-merge-argv.txt\"\n\
         cp \"$here/gh-stdin.txt\" \"$here/gh-merge-stdin.txt\"\n\
         echo merge >> \"$here/gh-merges.txt\"",
    );
    desk.gh_answers(&json!(""), 0, "");
    desk.output("merge", &["--change", CHANGE]);
    assert_eq!(desk.gh_file("gh-merge-stdin.txt"), "");
}

#[test]
fn a_number_github_does_not_know_is_refused_naming_it_and_a_known_one_merges() {
    let desk = Desk::new();
    change(&desk, "land", HEAD, TESTED);
    desk.gh_also(
        "if [ \"$1\" = api ]; then echo 'gh: Not Found (HTTP 404)' >&2; exit 1; fi\n\
         echo merge >> \"$here/gh-merges.txt\"",
    );
    let (ran, document) = desk.call("merge", &["--change", CHANGE, "--number", "99"]);
    assert_eq!(ran.code, 1, "{document}");
    assert_eq!(document["code"], "github::merge::pull");
    assert_eq!(
        document["message"],
        "GitHub has no pull request #99 in octo/widgets to take the commit's subject and body \
         from"
    );
    assert_eq!(document["fields"]["number"], 99);
    assert_eq!(
        document["recovery"]["text"],
        "check --number, or pass --subject and --body"
    );
    assert_eq!(merges(&desk), 0);
    pulls(&desk, true);
    desk.gh_answers(&json!(""), 0, "");
    let merged = desk.output("merge", &["--change", CHANGE, "--number", "12"]);
    assert_eq!(merged["phase"], "done");
}

#[test]
fn a_pull_request_without_a_title_is_refused_when_the_subject_is_left_out_and_merges_with_one() {
    let desk = Desk::new();
    change(&desk, "land", HEAD, TESTED);
    desk.gh_also(
        "if [ \"$1\" = api ]; then printf '%s' '{\"merged\":true,\"body\":\"Levers lift.\"}'; exit 0; fi\n\
         echo merge >> \"$here/gh-merges.txt\"",
    );
    let (ran, document) = desk.call("merge", &["--change", CHANGE]);
    assert_eq!(ran.code, 1, "{document}");
    assert_eq!(document["code"], "github::merge::pull");
    assert_eq!(
        document["message"],
        "GitHub gave pull request #12 in octo/widgets no title to take the commit's subject \
         from"
    );
    assert_eq!(document["recovery"]["text"], "pass --subject");
    assert_eq!(merges(&desk), 0);
    desk.gh_answers(&json!(""), 0, "");
    let merged = desk.output("merge", &["--change", CHANGE, "--subject", "Add a lever"]);
    assert_eq!(merged["phase"], "done");
    assert_eq!(merges(&desk), 1);
}
