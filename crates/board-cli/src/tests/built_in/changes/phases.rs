//! `test_phases.py`: `next`, `attach` and `advance` over a real board,
//! which read the gate from a change's references and move its phase.

use super::*;

const CHANGE: &str = "/changes/t";
pub(super) const MOCKUP: &str = "https://claude.ai/artifact/m1";

/// `{"path": CHANGE}` with `fields`.
fn at(fields: &Value) -> Value {
    serde_json::from_str(&record(json!({"path": CHANGE}), fields)).unwrap()
}

fn path() -> Value {
    at(&json!({}))
}

/// The plan input for [`CHANGE`] declaring `paths`.
fn planning(paths: &[&str]) -> Value {
    plan_input(CHANGE, paths)
}

/// Writes the owner's signed-off understanding check of /changes/t's data,
/// and returns the records to attach for it.
fn signed_data(board: &Changes) -> [&'static str; 2] {
    let understood = "/changes/t/understanding/data";
    board.write(
        "changes/t/understanding/data.json",
        &typed(
            "changes/understanding/v1",
            &json!({"change": CHANGE, "subject": "data",
                    "questions": [{"question": "Q?", "answer": "A.", "design": "A."}]}),
        ),
    );
    board.write(
        "decisions/data.json",
        &decision(&json!({"refs": [understood]})),
    );
    [understood, "/decisions/data"]
}

/// Makes the plan of /changes/t store data, so it needs the owner's
/// approval.
fn storing(board: &Changes) {
    let mut plan = board.read("changes/t/plans/1.json");
    plan["answers"] = json!({"ux": "none", "data": "stored", "lifecycle": "none"});
    board.write("changes/t/plans/1.json", &plan.to_string());
}

/// What a refusal of `advance` says is missing.
fn missing(board: &Changes) -> Value {
    board.refusal("advance", &path())["fields"]["missing"].clone()
}

/// The detail of the `paths` check `next` shows for /changes/t.
fn paths_detail(board: &Changes) -> String {
    let next = board.call("next", &path());
    let check = (next["gate"].as_array().unwrap().iter())
        .find(|each| each["check"] == "paths")
        .unwrap();
    assert_eq!(check["status"], "pass", "{check}");
    check["detail"].as_str().unwrap().to_owned()
}

/// The phase a successful `advance` moved the change to.
fn advanced(board: &Changes) -> Value {
    board.call("advance", &path())["to"].clone()
}

/// A change in review whose head edits /repos/x/src/a.rs, under its plan's
/// paths: the base and head.
fn reviewing(board: &Changes, records: &[&str]) -> (String, String) {
    let (base, head) = board.edited(&[("src/a.rs", "a\n")]);
    board.change_at(
        "changes/t.json",
        "review",
        &json!({"head": head, "records": records}),
    );
    board.call_stdin("plan", &planning(&["/repos/x/src/**"]));
    (base, head)
}

/// An approving report of `base..head`.
fn approving(head: &str, base: &str) -> String {
    report(head, "approve", base, &json!({}))
}

#[test]
fn next_reads_the_gate_from_the_plan_file_and_says_what_to_do() {
    let board = Changes::new();
    board.change_at(
        "changes/t.json",
        "plan",
        &json!({"plan": "/changes/t/plan.md", "route": "owner", "records": [board.approved()]}),
    );
    board.write(
        "changes/t/plan.md",
        "# T\n\n## Problem\n\n## Outcome\n\n## Out of scope\n",
    );
    let output = board.call("next", &path());
    assert_eq!(
        (&output["phase"], &output["route"]),
        (&json!("plan"), &json!("owner"))
    );
    let statuses: BTreeMap<&str, &str> = (output["gate"].as_array().unwrap().iter())
        .map(|check| {
            (
                check["check"].as_str().unwrap(),
                check["status"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        statuses,
        BTreeMap::from([
            ("plan-sections", "missing"),
            ("considered", "pass"),
            ("task", "pass"),
            ("route", "pass"),
            ("lifecycle-declared", "pass"),
            ("obligations", "pass"),
        ])
    );
    // The plan answers ux none and data none, so it needs no approval.
    let skipped = output["not_required"].as_array().unwrap();
    assert!(skipped.contains(&json!("owner-approval")), "{skipped:?}");
    assert_eq!(
        output["flow"],
        json!({"name": "development", "phases": ["plan", "build", "review", "land"]})
    );
    let instruction = output["instruction"].as_str().unwrap();
    assert!(
        instruction.starts_with("Read each record the change surfaced"),
        "{instruction}"
    );
    board.write("changes/t/plan.md", PLAN);
    let output = board.call("next", &path());
    assert_eq!(
        output["instruction"],
        "The gate passes: run `board changes advance --path /changes/t`."
    );
}

#[test]
fn advance_refuses_an_unmet_gate_naming_what_is_missing_and_moves_on_once_it_is_met() {
    let board = Changes::new();
    let data = signed_data(&board);
    board.change_at(
        "changes/t.json",
        "plan",
        &json!({"plan": "/changes/t/plan.md", "route": "owner", "records": data}),
    );
    storing(&board);
    board.write("changes/t/plan.md", "# T\n");
    let refused = board.refusal("advance", &path());
    assert_eq!(refused["code"], "changes::gate::unmet");
    assert_eq!(
        refused["fields"]["missing"],
        json!(["plan-sections", "owner-approval"])
    );
    let remedies = refused["fields"]["remedies"].as_array().unwrap();
    assert_eq!(remedies.len(), 2);
    assert!(
        remedies[1]
            .as_str()
            .unwrap()
            .starts_with("Ask the owner with the question `board changes ask` files"),
        "{remedies:?}"
    );
    board.change_at(
        "changes/t.json",
        "plan",
        &json!({"plan": "/changes/t/plan.md", "route": "owner",
                "records": [board.approved(), data[0], data[1]]}),
    );
    storing(&board);
    assert_eq!(board.read("changes/t.json")["phase"], "plan");
    board.write("changes/t/plan.md", PLAN);
    assert_eq!(
        board.call("advance", &path()),
        json!({"path": CHANGE, "from": "plan", "to": "build"})
    );
    assert_eq!(board.read("changes/t.json")["phase"], "build");
}

#[test]
fn advance_refuses_a_plan_only_an_agent_approved_and_moves_on_once_a_decision_names_a_person() {
    let board = Changes::new();
    let agent = json!({"refs": [CHANGE], "by": {"program": "claude-code", "session": "s"}});
    board.write("decisions/agent.json", &decision(&agent));
    let [understood, signed] = signed_data(&board);
    board.change_at(
        "changes/t.json",
        "plan",
        &json!({"plan": "/changes/t/plan.md", "route": "owner",
                "records": ["/decisions/agent", understood, signed]}),
    );
    storing(&board);
    board.write("changes/t/plan.md", PLAN);
    let refused = board.refusal("advance", &path());
    assert_eq!(
        (&refused["code"], &refused["fields"]["missing"]),
        (&json!("changes::gate::unmet"), &json!(["owner-approval"]))
    );
    board.call("attach", &at(&json!({"ref": board.approved()})));
    assert_eq!(advanced(&board), "build");
}

#[test]
fn a_decision_the_plan_only_uses_approves_nothing_and_attaching_it_does() {
    let board = Changes::new();
    board.write(
        "decisions/owner.json",
        &decision(&json!({"refs": [CHANGE], "decides": "/changes/t/plans/1"})),
    );
    let data = signed_data(&board);
    board.change_at(
        "changes/t.json",
        "plan",
        &json!({"plan": "/changes/t/plan.md", "route": "owner", "records": data}),
    );
    storing(&board);
    board.write("changes/t/plan.md", PLAN);
    let mut plan = board.read("changes/t/plans/1.json");
    plan["uses"] = json!(["/decisions/owner"]);
    board.write("changes/t/plans/1.json", &plan.to_string());
    assert_eq!(missing(&board), json!(["owner-approval"]));
    board.call("attach", &at(&json!({"ref": "/decisions/owner"})));
    assert_eq!(advanced(&board), "build");
}

#[test]
fn a_degraded_referenced_record_satisfies_nothing_and_a_valid_one_does() {
    let board = Changes::new();
    let (base, head) = reviewing(&board, &["/r/1"]);
    board.write("r/1.json", "{not json");
    assert_eq!(missing(&board), json!(["review"]));
    board.write("r/1.json", &approving(&head, &base));
    assert_eq!(advanced(&board), "land");
}

#[test]
fn a_referenced_record_degraded_only_by_a_dangling_reference_still_counts() {
    let board = Changes::new();
    let (base, head) = reviewing(&board, &["/r/1"]);
    board.write(
        "r/1.json",
        &report(&head, "changes", &base, &json!({"brief": "/briefs/none"})),
    );
    assert_eq!(missing(&board), json!(["review"]));
    board.write(
        "r/1.json",
        &report(&head, "approve", &base, &json!({"brief": "/briefs/none"})),
    );
    assert_eq!(board.code("/r/1").as_deref(), Some("board::ref::dangling"));
    assert_eq!(advanced(&board), "land");
}

#[test]
fn a_referenced_record_deleted_after_it_was_attached_satisfies_nothing() {
    let board = Changes::new();
    let (base, head) = reviewing(&board, &[]);
    board.write("r/1.json", &approving(&head, &base));
    board.call("attach", &at(&json!({"ref": "/r/1"})));
    board.delete("r/1.json");
    assert_eq!(missing(&board), json!(["review"]));
    board.write("r/1.json", &approving(&head, &base));
    assert_eq!(advanced(&board), "land");
}

#[test]
fn a_review_attached_before_the_plan_is_revised_still_passes_the_gate_and_is_cited_in_the_pr() {
    let board = Changes::new();
    let (base, head) = reviewing(&board, &[]);
    board.write("r/1.json", &approving(&head, &base));
    board.call("attach", &at(&json!({"ref": "/r/1"})));
    board.call_stdin("plan", &planning(&["/repos/x/src/**"]));
    assert_eq!(advanced(&board), "land");
    board.call("pr", &path());
    let body = board.text("changes/t/pr.md");
    assert!(
        body.contains(&format!(
            "- `/r/1`: approve by `a reviewer` over {base}..{head}, findings none"
        )),
        "{body}"
    );
}

#[test]
fn review_passes_a_diff_that_edits_a_file_outside_the_planned_paths_and_the_body_lists_it() {
    let board = Changes::new();
    let (base, head) = board.edited(&[("src/a.rs", "a\n"), ("docs/b.md", "b\n")]);
    board.change_at(
        "changes/t.json",
        "review",
        &json!({"head": head, "records": ["/r/1"]}),
    );
    board.write("r/1.json", &approving(&head, &base));
    board.call_stdin("plan", &planning(&["/repos/x/src/**"]));
    let detail = paths_detail(&board);
    assert!(detail.contains("/repos/x/docs/b.md"), "{detail}");
    assert!(!detail.contains("/repos/x/src/a.rs"), "{detail}");
    assert_eq!(advanced(&board), "land");
    board.call("pr", &path());
    let body = board.text("changes/t/pr.md");
    assert!(
        body.contains("## Files added to the plan's paths"),
        "{body}"
    );
    assert!(body.contains("- `/repos/x/docs/b.md`"), "{body}");
    assert!(!body.contains("- `/repos/x/src/a.rs`"), "{body}");
}

#[test]
fn review_checks_every_commit_since_the_followed_branch_when_a_report_covers_only_the_fix() {
    let board = Changes::new();
    let (_, first) = board.edited(&[("docs/b.md", "b\n")]);
    let head = board.commit(&[("src/a.rs", "a\n")], None);
    board.change_at(
        "changes/t.json",
        "review",
        &json!({"head": head, "records": ["/r/1"]}),
    );
    board.write("r/1.json", &approving(&head, &first));
    board.call_stdin("plan", &planning(&["/repos/x/src/**"]));
    let detail = paths_detail(&board);
    assert!(detail.contains("/repos/x/docs/b.md"), "{detail}");
    assert_eq!(advanced(&board), "land");
}

#[test]
fn review_leaves_out_what_the_followed_branch_gained_when_the_change_is_rebased_on_it() {
    let board = Changes::new();
    let (old, _) = board.edited(&[("src/a.rs", "a\n")]);
    let root = board.repo_root("repos/x");
    board.commit(&[("docs/upstream.md", "u\n")], Some("main"));
    git(&root, &["checkout", "-q", "work"]);
    git(&root, &["rebase", "-q", "main"]);
    let head = git(&root, &["rev-parse", "HEAD"]);
    board.change_at(
        "changes/t.json",
        "review",
        &json!({"head": head, "records": ["/r/1"]}),
    );
    board.write("r/1.json", &approving(&head, &old));
    board.call_stdin("plan", &planning(&["/repos/x/src/**"]));
    assert_eq!(advanced(&board), "land");
    board.commit(&[("docs/mine.md", "m\n")], None);
    let head = git(&root, &["rev-parse", "HEAD"]);
    board.change_at(
        "changes/t.json",
        "review",
        &json!({"head": head, "records": ["/r/1"]}),
    );
    board.write("r/1.json", &approving(&head, &old));
    let detail = paths_detail(&board);
    assert!(detail.contains("/repos/x/docs/mine.md"), "{detail}");
    assert!(!detail.contains("/repos/x/docs/upstream.md"), "{detail}");
}

#[test]
fn the_build_gate_finds_the_check_suite_in_the_repositorys_contributing_docs() {
    let board = Changes::new();
    board.mount_x(&[("CONTRIBUTING.md", "Checks: `/suites/checks`.\n")]);
    board.write("suites/checks.json", &typed("runs/suite/v1", &json!({})));
    let run = json!({
        "suite": "/suites/checks",
        "scope": "full",
        "dirty": false,
        "versions": {"x": HEAD},
        "results": [],
    });
    board.write("suites/checks/runs/1.json", &typed("runs/run/v1", &run));
    board.change_at(
        "changes/t.json",
        "build",
        &json!({"head": HEAD, "plan": "/changes/t/plan.md", "records": ["/suites/checks/runs/1"]}),
    );
    board.write("changes/t/plan.md", PLAN);
    assert_eq!(advanced(&board), "review");
}

#[test]
fn the_build_gate_counts_a_run_of_a_suite_in_the_repositorys_own_board_by_where_the_run_lives() {
    let board = Changes::new();
    let run = json!({"scope": "full", "dirty": false, "versions": {"x": HEAD}, "results": []});
    let suite = typed("runs/suite/v1", &json!({}));
    let checks_run = typed("runs/run/v1", &run);
    let other_run = typed(
        "runs/run/v1",
        &serde_json::from_str(&record(
            run.clone(),
            &json!({"suite": "/repos/x/suites/checks"}),
        ))
        .unwrap(),
    );
    board.mount_x(&[
        ("CONTRIBUTING.md", "Checks: `/suites/checks`.\n"),
        (".board/types/runs/suite/v1.schema.json", "{}"),
        (".board/types/runs/run/v1.schema.json", "{}"),
        ("suites/checks.json", &suite),
        ("suites/checks/runs/1.json", &checks_run),
        ("suites/other.json", &suite),
        ("suites/other/runs/1.json", &other_run),
    ]);
    board.change_at(
        "changes/t.json",
        "build",
        &json!({"head": HEAD, "plan": "/changes/t/plan.md", "records": ["/repos/x/suites/other/runs/1"]}),
    );
    board.write("changes/t/plan.md", PLAN);
    assert_eq!(missing(&board), json!(["suite-run"]));
    board.call(
        "attach",
        &at(&json!({"ref": "/repos/x/suites/checks/runs/1"})),
    );
    assert_eq!(advanced(&board), "review");
}

#[test]
fn next_on_a_done_change_has_no_gate_and_nothing_to_do() {
    let board = Changes::new();
    board.change_at("changes/t.json", "done", &json!({}));
    let output = board.call("next", &path());
    assert_eq!(
        (&output["phase"], &output["gate"], &output["instruction"]),
        (&json!("done"), &json!([]), &json!("Nothing is left to do."))
    );
}

/// The page of pull request 1 of o/r, which [`landing`] records.
const PULL: &str = "https://github.com/o/r/pull/1";
/// The commit GitHub gives as pull request 1's merge.
const MERGED: &str = "0123456789abcdef0123456789abcdef01234567";

/// A board with the stub gh and /changes/t in land, with `fields`, whose
/// plan holds the sections the land gate asks for.
fn landing(fields: &Value) -> Changes {
    let board = gh();
    let given = record(json!({"plan": "/changes/t/plan.md"}), fields);
    board.change_at(
        "changes/t.json",
        "land",
        &serde_json::from_str(&given).unwrap(),
    );
    board.write(
        "changes/t/plan.md",
        "# T\n\n## What the tests prove\n\nIt lands.\n\n## What the tests do not prove\n\nRetries.\n",
    );
    board
}

#[test]
fn advance_from_land_is_refused_while_the_pull_request_is_open_and_moves_to_done_once_it_merged() {
    let board = landing(&json!({"pull_request": PULL}));
    board.answers(1, &json!({"state": "open", "merged": false}));
    let refused = board.refusal("advance", &path());
    assert_eq!(
        (&refused["code"], &refused["fields"]["pull_request"]),
        (&json!("changes::land::not_merged"), &json!(PULL))
    );
    let message = refused["message"].as_str().unwrap();
    assert!(
        message.contains("board github merge --change /changes/t"),
        "{message}"
    );
    assert_eq!(board.read("changes/t.json")["phase"], "land");
    board.answers(
        1,
        &json!({"state": "closed", "merged": true, "merge_commit_sha": MERGED}),
    );
    assert_eq!(advanced(&board), "done");
    assert_eq!(
        board.read("changes/t.json")["merge_commit_url"],
        format!("https://github.com/o/r/commit/{MERGED}")
    );
    assert_eq!(board.gh_calls(), ["api repos/o/r/pulls/1"; 2]);
}

#[test]
fn advance_from_land_is_refused_for_a_pull_request_closed_unmerged_and_one_github_cannot_find() {
    let board = landing(&json!({"pull_request": PULL}));
    board.answers(1, &json!({"state": "closed", "merged": false}));
    let refused = board.refusal("advance", &path());
    assert_eq!(refused["code"], "changes::land::not_merged");
    let message = refused["message"].as_str().unwrap();
    assert!(
        message.contains("board changes drop --path /changes/t"),
        "{message}"
    );
    board.bin.write("pulls/1.json", Item::Deleted);
    let refused = board.refusal("advance", &path());
    assert_eq!(refused["code"], "changes::land::not_merged");
    let message = refused["message"].as_str().unwrap();
    assert!(message.contains("404"), "{message}");
    assert_eq!(board.read("changes/t.json")["phase"], "land");
    board.answers(1, &json!({"state": "closed", "merged": true}));
    assert_eq!(advanced(&board), "done");
}

#[test]
fn advance_from_land_is_refused_when_gh_fails_and_moves_once_gh_answers() {
    let board = landing(&json!({"pull_request": PULL}));
    board.answers(1, &json!({"state": "closed", "merged": true}));
    board.bin.write("fail", Item::Fresh(""));
    let refused = board.refusal("advance", &path());
    assert_eq!(refused["code"], "changes::land::gh_failed");
    let message = refused["message"].as_str().unwrap();
    assert!(message.contains("HTTP 502"), "{message}");
    assert_eq!(board.read("changes/t.json")["phase"], "land");
    board.bin.write("fail", Item::Deleted);
    assert_eq!(advanced(&board), "done");
}

#[test]
fn attach_refuses_a_reference_that_reaches_nothing_and_adds_one_that_does_once() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({"records": []}));
    board.write("tasks/a.md", &task("open"));
    let refused = board.refusal("attach", &at(&json!({"ref": "/tasks/b.md"})));
    assert_eq!(
        (&refused["code"], &refused["fields"]["ref"]),
        (&json!("changes::ref::dangling"), &json!("/tasks/b.md"))
    );
    for _ in 0..2 {
        assert_eq!(
            board.call("attach", &at(&json!({"ref": "/tasks/a.md"}))),
            json!({"path": CHANGE, "records": 1})
        );
    }
    assert_eq!(
        board.read("changes/t/plans/1.json")["records"],
        json!(["/tasks/a.md"])
    );
}

#[test]
fn attach_refuses_a_reference_for_a_change_with_no_plan_record_and_keeps_it_on_the_plan_once_written()
 {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    board.write("tasks/a.md", &task("open"));
    let refused = board.refusal("attach", &at(&json!({"ref": "/tasks/a.md"})));
    assert_eq!(
        (&refused["code"], &refused["fields"]["path"]),
        (&json!("changes::attach::no_plan"), &json!(CHANGE))
    );
    let message = refused["message"].as_str().unwrap();
    assert!(
        message.contains("changes plan --path /changes/t"),
        "{message}"
    );
    let written = board.call_stdin("plan", &planning(&["/repos/x/**"]));
    assert_eq!(
        board.call("attach", &at(&json!({"ref": "/tasks/a.md"}))),
        json!({"path": CHANGE, "records": 1})
    );
    let plan = written["path"].as_str().unwrap().trim_start_matches('/');
    assert_eq!(
        board.read(&format!("{plan}.json"))["records"],
        json!(["/tasks/a.md"])
    );
}

#[test]
fn attach_sets_the_route_head_ticket_and_pull() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    let fields = |number: u64, kind: &str| {
        json!({
            "owner": "o",
            "repo": "r",
            "number": number,
            "url": format!("https://github.com/o/r/{kind}/{number}"),
        })
    };
    board.write(
        "gh/o/r/pulls/1.json",
        &typed("github/pull/v1", &fields(1, "pull")),
    );
    board.write(
        "gh/o/r/issues/2.json",
        &typed("github/issue/v1", &fields(2, "issues")),
    );
    board.call(
        "attach",
        &at(&json!({"route": "owner", "head": HEAD, "pull": "/gh/o/r/pulls/1"})),
    );
    let written = board.read("changes/t.json");
    assert_eq!(
        (&written["route"], &written["head"], &written["pulls"]),
        (&json!("owner"), &json!(HEAD), &json!(["/gh/o/r/pulls/1"]))
    );
    board.call("attach", &at(&json!({"ticket": "/gh/o/r/issues/2"})));
    assert_eq!(
        board.read("changes/t.json")["tickets"],
        json!(["/gh/o/r/issues/2"])
    );
}

#[test]
fn attach_with_nothing_to_change_is_refused_by_its_input_schema_and_with_one_thing_is_admitted() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    let refused = board.refusal("attach", &path());
    assert_eq!(refused["code"], "board::plugin::bad_input");
    assert_eq!(
        board.call("attach", &at(&json!({"route": "owner"})))["records"],
        0
    );
}

#[test]
fn a_path_that_holds_no_change_is_refused_and_one_that_does_is_answered() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    board.write("tasks/a.md", &task("open"));
    for capability in ["next", "attach", "advance"] {
        let given = if capability == "attach" {
            json!({"path": "/tasks/a.md", "route": "owner"})
        } else {
            json!({"path": "/tasks/a.md"})
        };
        assert_eq!(
            board.refusal(capability, &given)["code"],
            "changes::change::not_found",
            "{capability}"
        );
    }
    assert_eq!(board.call("next", &path())["phase"], "plan");
}

#[test]
fn a_change_board_marks_degraded_is_refused_and_a_valid_one_is_answered() {
    let board = Changes::new();
    board.change_at("changes/t.json", "later", &json!({}));
    assert_eq!(
        board.refusal("next", &path())["code"],
        "changes::change::degraded"
    );
    board.change_at("changes/t.json", "plan", &json!({}));
    assert_eq!(board.call("next", &path())["phase"], "plan");
}

#[test]
fn attach_refuses_a_ticket_or_pull_that_reaches_nothing_and_adds_one_that_does_once() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    board.write("tickets/ACME-123.json", &typed("jira/issue/v1", &json!({})));
    board.write(
        "pulls/octo-widgets-7.json",
        &typed("github/pull/v1", &json!({})),
    );
    for (field, target) in [
        ("ticket", "/tickets/ACME-124"),
        ("pull", "/pulls/octo-widgets-8"),
    ] {
        let refused = board.refusal("attach", &at(&json!({field: target})));
        assert_eq!(
            (&refused["code"], &refused["fields"]["ref"]),
            (&json!("changes::ref::dangling"), &json!(target)),
            "{field}"
        );
    }
    for _ in 0..2 {
        board.call(
            "attach",
            &at(&json!({"ticket": "/tickets/ACME-123", "pull": "/pulls/octo-widgets-7"})),
        );
    }
    let written = board.read("changes/t.json");
    assert_eq!(
        (&written["tickets"], &written["pulls"]),
        (
            &json!(["/tickets/ACME-123"]),
            &json!(["/pulls/octo-widgets-7"])
        )
    );
}

#[test]
fn attach_refuses_a_ticket_or_pull_of_another_type_and_adds_each_type_it_accepts() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    board.write("tickets/ACME-123.json", &typed("jira/issue/v1", &json!({})));
    board.write(
        "issues/octo-widgets-6.json",
        &typed("github/issue/v1", &json!({})),
    );
    board.write(
        "pulls/octo-widgets-7.json",
        &typed("github/pull/v1", &json!({})),
    );
    for (field, target) in [
        ("ticket", "/pulls/octo-widgets-7"),
        ("pull", "/issues/octo-widgets-6"),
    ] {
        let refused = board.refusal("attach", &at(&json!({field: target})));
        assert_eq!(
            (&refused["code"], &refused["fields"]["ref"]),
            (&json!("changes::ref::wrong_type"), &json!(target)),
            "{field}"
        );
    }
    board.call("attach", &at(&json!({"ticket": "/tickets/ACME-123"})));
    board.call(
        "attach",
        &at(&json!({"ticket": "/issues/octo-widgets-6", "pull": "/pulls/octo-widgets-7"})),
    );
    let written = board.read("changes/t.json");
    assert_eq!(
        written["tickets"],
        json!(["/tickets/ACME-123", "/issues/octo-widgets-6"])
    );
    assert_eq!(written["pulls"], json!(["/pulls/octo-widgets-7"]));
}

#[test]
fn attach_refuses_a_ticket_that_is_a_markdown_file_and_adds_a_ticket_record() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    board.write("tickets/ACME-123.md", "# ACME-123\n");
    board.write("tickets/ACME-123.json", &typed("jira/issue/v1", &json!({})));
    let refused = board.refusal("attach", &at(&json!({"ticket": "/tickets/ACME-123.md"})));
    assert_eq!(
        (&refused["code"], &refused["fields"]["type"]),
        (&json!("changes::ref::wrong_type"), &Value::Null)
    );
    let message = refused["message"].as_str().unwrap();
    assert!(message.contains("is not a record"), "{message}");
    board.call("attach", &at(&json!({"ticket": "/tickets/ACME-123"})));
    assert_eq!(
        board.read("changes/t.json")["tickets"],
        json!(["/tickets/ACME-123"])
    );
}

#[test]
fn a_change_with_no_task_blocks_the_plan_gate_and_attaching_one_clears_it() {
    let board = Changes::new();
    board.write(
        "changes/t/plans/1.json",
        &typed(
            "changes/plan/v1",
            &json!({
                "text": "/changes/t/plan.md",
                "uses": [],
                "answers": answers(),
                "records": [board.approved()],
            }),
        ),
    );
    board.write(
        "changes/t.json",
        &typed(
            "changes/change/v1",
            &json!({
                "title": "T",
                "mount": "/repos/x",
                "phase": "plan",
                "plan": "/changes/t/plans/1",
                "route": "owner",
            }),
        ),
    );
    board.write("changes/t/plan.md", PLAN);
    assert_eq!(missing(&board), json!(["task"]));
    board.write(
        "notes/a.json",
        &typed("plans/decision/v1", &json!({"refs": []})),
    );
    assert_eq!(
        board.refusal("attach", &at(&json!({"task": "/notes/a"})))["code"],
        "changes::ref::wrong_type"
    );
    board.call("attach", &at(&json!({"task": TASK})));
    assert_eq!(advanced(&board), "build");
}

/// The fixture plan with these UX and Data sections.
pub(super) fn ux_plan(ux: &str, data: &str) -> String {
    let before = PLAN.split("## UX").next().unwrap();
    let (_, after) = PLAN.split_once("## Failure paths").unwrap();
    format!("{before}## UX\n\n{ux}\n\n## Data\n\n{data}\n\n## Failure paths{after}")
}

/// An owner-routed change in plan whose plan file holds `plan` and whose
/// plan answers `ux` and `data` with these words.
pub(super) fn planned_change(board: &Changes, plan: &str, ux: &str, data: &str) {
    board.change_at(
        "changes/t.json",
        "plan",
        &json!({"plan": "/changes/t/plan.md", "route": "owner", "records": [board.approved()]}),
    );
    board.write("changes/t/plan.md", plan);
    let mut record = board.read("changes/t/plans/1.json");
    record["answers"] = json!({"ux": ux, "data": data, "lifecycle": "none"});
    board.write("changes/t/plans/1.json", &record.to_string());
}

/// A decision of the owner's, whose `by` names a person, with these refs,
/// urls or other fields.
pub(super) fn decision(fields: &Value) -> String {
    let body = json!({
        "$type": "plans/decision/v1",
        "decision": "Agreed.",
        "why": "Seen.",
        "session": "s",
        "decided": "2026-11-01T10:00:00Z",
        "by": {"person": PERSON},
    });
    record(body, fields)
}

#[test]
fn advance_refuses_a_mockup_the_owner_has_not_agreed_and_moves_on_once_a_decision_names_it() {
    let board = Changes::new();
    planned_change(
        &board,
        &ux_plan(
            &format!(
                "### Mockups\n\n- {MOCKUP}, version v1\n\n### Neighbours\n\n- /changes/other\n"
            ),
            "None.",
        ),
        "mockups",
        "none",
    );
    assert_eq!(
        board.refusal("advance", &path())["code"],
        "changes::ux::not_agreed"
    );
    board.write("decisions/ux.json", &decision(&json!({"urls": [MOCKUP]})));
    board.call("attach", &at(&json!({"ref": "/decisions/ux"})));
    assert_eq!(advanced(&board), "build");
}

#[test]
fn advance_refuses_a_mockup_the_owner_has_not_agreed_and_moves_on_once_his_answer_names_it() {
    let board = Changes::new();
    planned_change(
        &board,
        &ux_plan(
            &format!(
                "### Mockups\n\n- {MOCKUP}, version v1\n\n### Neighbours\n\n- /changes/other\n"
            ),
            "None.",
        ),
        "mockups",
        "none",
    );
    for kind in ["question", "answer"] {
        board.write(
            &format!(".board/types/plans/{kind}/v1.schema.json"),
            r#"{"type": "object"}"#,
        );
    }
    let options = json!([
        {"id": "agree", "text": format!("Agree {MOCKUP} version v1 as the mockup")},
        {"id": "change", "text": "Change something"},
    ]);
    let question = json!({
        "$type": "plans/question/v1", "question": "Agree the mockup?", "options": options,
        "by": {"session": "s"}, "asked": "2026-11-01T10:00:00Z", "status": "open",
    });
    board.write("questions/ux.json", &question.to_string());
    let answer = json!({
        "$type": "plans/answer/v1", "question": "/questions/ux", "choice": "agree",
        "words": "Agree it.", "answered": "2026-11-01T10:01:00Z", "by": {"person": PERSON},
    });
    board.write("answers/ux.json", &answer.to_string());
    board.call("attach", &at(&json!({"ref": "/answers/ux"})));
    assert_eq!(
        board.refusal("advance", &path())["code"],
        "changes::ux::not_agreed"
    );
    board.call("attach", &at(&json!({"ref": "/questions/ux"})));
    assert_eq!(advanced(&board), "build");
}

#[test]
fn advance_refuses_mockups_with_no_neighbours_list_and_moves_on_once_one_is_written() {
    let board = Changes::new();
    board.write("decisions/ux.json", &decision(&json!({"urls": [MOCKUP]})));
    planned_change(
        &board,
        &ux_plan(&format!("### Mockups\n\n- {MOCKUP}, version v1"), "None."),
        "mockups",
        "none",
    );
    board.call("attach", &at(&json!({"ref": "/decisions/ux"})));
    assert_eq!(
        board.refusal("advance", &path())["code"],
        "changes::ux::no_neighbours"
    );
    board.write(
        "changes/t/plan.md",
        &ux_plan(
            &format!(
                "### Mockups\n\n- {MOCKUP}, version v1\n\n### Neighbours\n\n- /changes/other\n"
            ),
            "None.",
        ),
    );
    assert_eq!(advanced(&board), "build");
}

#[test]
fn advance_refuses_a_mockup_with_no_version_and_moves_on_once_the_plan_names_one() {
    let board = Changes::new();
    board.write("decisions/ux.json", &decision(&json!({"urls": [MOCKUP]})));
    let listed = |item: &str| {
        ux_plan(
            &format!("### Mockups\n\n- {item}\n\n### Neighbours\n\n- /changes/other\n"),
            "None.",
        )
    };
    planned_change(&board, &listed(MOCKUP), "mockups", "none");
    board.call("attach", &at(&json!({"ref": "/decisions/ux"})));
    assert_eq!(
        board.refusal("advance", &path())["code"],
        "changes::ux::no_version"
    );
    board.write(
        "changes/t/plan.md",
        &listed(&format!("{MOCKUP}, version v1")),
    );
    assert_eq!(advanced(&board), "build");
}

#[test]
fn advance_refuses_a_review_with_no_conformance_record_and_moves_on_once_one_matches() {
    let board = Changes::new();
    let (base, head) = reviewing(&board, &[]);
    let plan = ux_plan(
        &format!("### Mockups\n\n- {MOCKUP}, version v1\n\n### Neighbours\n\n- /changes/other\n"),
        "None.",
    );
    let stored = board.read("changes/t.json")["plan"]
        .as_str()
        .unwrap()
        .trim_start_matches('/')
        .to_owned();
    let mut record = board.read(&format!("{stored}.json"));
    board.write(
        record["text"].as_str().unwrap().trim_start_matches('/'),
        &plan,
    );
    record["answers"] = json!({"ux": "mockups", "data": "none", "lifecycle": "none"});
    board.write(&format!("{stored}.json"), &record.to_string());
    board.write("decisions/ux.json", &decision(&json!({"urls": [MOCKUP]})));
    board.write("r/1.json", &approving(&head, &base));
    board.call("attach", &at(&json!({"ref": "/decisions/ux"})));
    board.call("attach", &at(&json!({"ref": "/r/1"})));
    assert_eq!(
        board.refusal("advance", &path())["code"],
        "changes::ux::not_conformed"
    );
    let conformance = json!({
        "$type": "changes/conformance/v1", "change": CHANGE, "mockup": MOCKUP,
        "version": "v1", "head": board.read("changes/t.json")["head"], "page": "http://localhost:3000/",
        "at": "2026-11-19T10:00:00Z", "verdict": "match", "differences": [],
    });
    board.write("changes/t/conformance/1.json", &conformance.to_string());
    board.call("attach", &at(&json!({"ref": "/changes/t/conformance/1"})));
    assert_eq!(advanced(&board), "land");
}

/// An understanding check of /changes/t's data, every question answered
/// as the plan has it.
fn understanding_record() -> Value {
    json!({
        "$type": "changes/understanding/v1",
        "change": CHANGE,
        "subject": "data",
        "questions": [
            {
                "question": "How many records does one call write?",
                "answer": "One, I think.",
                "design": "One record per call.",
                "kind": "predict",
                "matches": true,
            },
        ],
    })
}

#[test]
fn advance_refuses_unsigned_data_and_moves_on_once_a_decision_references_the_understanding_check() {
    let board = Changes::new();
    planned_change(
        &board,
        &ux_plan(
            "No UI change: it is headless.",
            "One record per call, in the calls table.",
        ),
        "none",
        "stored",
    );
    assert_eq!(
        board.refusal("advance", &path())["code"],
        "changes::data::not_signed"
    );
    board.write(
        "changes/t/understanding/data.json",
        &understanding_record().to_string(),
    );
    board.write(
        "decisions/data.json",
        &decision(&json!({"refs": ["/changes/t/understanding/data"]})),
    );
    board.call(
        "attach",
        &at(&json!({"ref": "/changes/t/understanding/data"})),
    );
    board.call("attach", &at(&json!({"ref": "/decisions/data"})));
    assert_eq!(advanced(&board), "build");
}

#[test]
fn the_understanding_schema_refuses_a_question_without_the_designs_answer_and_admits_one_with_it() {
    let board = Changes::new();
    board.mount_x(&[("README.md", "x\n")]);
    board.change_at("changes/t.json", "plan", &json!({}));
    let asked = json!({"question": "Q?", "answer": "In my words."});
    let answered: Value =
        serde_json::from_str(&record(asked.clone(), &json!({"design": "The design's."}))).unwrap();
    board.write(
        "u/unanswered.json",
        &record(understanding_record(), &json!({"questions": [asked]})),
    );
    board.write(
        "u/answered.json",
        &record(understanding_record(), &json!({"questions": [answered]})),
    );
    assert_eq!(
        board.checked("/u/"),
        codes(&[
            ("/u/unanswered", Some("board::record::invalid")),
            ("/u/answered", None),
        ])
    );
}

#[test]
fn review_refuses_a_change_with_no_head_until_one_is_attached() {
    let board = Changes::new();
    let (base, head) = board.edited(&[("src/a.rs", "a\n")]);
    board.change_at("changes/t.json", "review", &json!({"records": ["/r/1"]}));
    board.write("r/1.json", &approving(&head, &base));
    board.call_stdin("plan", &planning(&["/repos/x/src/**"]));
    let refused = board.refusal("advance", &path());
    assert_eq!(
        (&refused["code"], &refused["fields"]["missing"]),
        (&json!("changes::gate::unmet"), &json!(["review", "paths"]))
    );
    let message = refused["message"].as_str().unwrap();
    assert!(
        message.contains("paths: the change records no head commit, so its diff is not known"),
        "{message}"
    );
    board.call("attach", &at(&json!({"head": head})));
    assert_eq!(advanced(&board), "land");
}

/// A change in review of a head under its plan's paths, with an approving
/// report, rewritten to name `mount`: the head and what refusing it says.
fn reviewed_at(board: &Changes, mount: &str) -> (String, String) {
    let (base, head) = board.edited(&[("src/a.rs", "a\n")]);
    board.change_at(
        "changes/t.json",
        "review",
        &json!({"head": head, "records": ["/r/1"]}),
    );
    board.write("r/1.json", &approving(&head, &base));
    board.call_stdin("plan", &planning(&["/repos/x/src/**"]));
    board.change_at(
        "changes/t.json",
        "review",
        &json!({"head": head, "records": ["/r/1"], "mount": mount}),
    );
    let refused = board.refusal("advance", &path());
    assert_eq!(refused["code"], "changes::paths::outside_plan");
    (head, refused["message"].as_str().unwrap().to_owned())
}

#[test]
fn review_refuses_a_change_whose_mount_holds_nothing_until_it_names_the_mount() {
    let board = Changes::new();
    let (head, message) = reviewed_at(&board, "/repos/gone");
    assert!(message.contains("nothing is at /repos/gone"), "{message}");
    board.change_at(
        "changes/t.json",
        "review",
        &json!({"head": head, "records": ["/r/1"]}),
    );
    assert_eq!(advanced(&board), "land");
}

#[test]
fn review_refuses_a_change_whose_mount_is_a_plain_directory_until_it_names_a_mount() {
    let board = Changes::new();
    let (head, message) = reviewed_at(&board, "/r");
    assert!(message.contains("/r is not a mount"), "{message}");
    board.change_at(
        "changes/t.json",
        "review",
        &json!({"head": head, "records": ["/r/1"]}),
    );
    assert_eq!(advanced(&board), "land");
}

#[test]
fn review_refuses_a_head_the_mounted_repository_lacks_until_the_head_is_one_it_holds() {
    let board = Changes::new();
    let (base, head) = board.edited(&[("src/a.rs", "a\n")]);
    let missing = "e".repeat(40);
    board.change_at(
        "changes/t.json",
        "review",
        &json!({"head": missing, "records": ["/r/1"]}),
    );
    board.write("r/1.json", &approving(&missing, &base));
    board.call_stdin("plan", &planning(&["/repos/x/src/**"]));
    let refused = board.refusal("advance", &path());
    assert_eq!(refused["code"], "changes::paths::outside_plan");
    let message = refused["message"].as_str().unwrap();
    assert!(
        message.contains(&format!(
            "fetch {missing} and main into the mounted repository"
        )),
        "{message}"
    );
    board.change_at(
        "changes/t.json",
        "review",
        &json!({"head": head, "records": ["/r/1"]}),
    );
    board.write("r/1.json", &approving(&head, &base));
    assert_eq!(advanced(&board), "land");
}

/// A change in review that passes every other review check, with an
/// obligation control on its planned paths.
fn reviewing_with_an_obligation(board: &Changes) {
    let (base, head) = reviewing(board, &["/r/1"]);
    board.write("r/1.json", &approving(&head, &base));
    let owed = json!({
        "$type": "controls/control/v1",
        "kind": "obligation",
        "requirement": "Sweep the docs.",
        "purpose": "p",
        "paths": ["repos/x/**"],
    });
    board.write("controls/x/docs.json", &owed.to_string());
}

#[test]
fn review_refuses_an_obligation_on_the_planned_paths_while_the_controls_plugin_is_enabled() {
    let board = Changes::new();
    reviewing_with_an_obligation(&board);
    let refused = board.refusal("advance", &path());
    assert_eq!(refused["code"], "changes::gate::unmet");
    assert_eq!(refused["fields"]["missing"], json!(["obligations-settled"]));
}

/// A change in review, passing every other review check, whose diff edits
/// `added` outside its planned paths, with an obligation control whose
/// fields beside its kind and text are `reach`.
fn adding_a_file_under_an_obligation(board: &Changes, added: &str, reach: &Value) {
    let (base, head) = board.edited(&[("src/a.rs", "a\n"), (added, "b\n")]);
    board.change_at(
        "changes/t.json",
        "review",
        &json!({"head": head, "records": ["/r/1"]}),
    );
    board.write("r/1.json", &approving(&head, &base));
    board.call_stdin("plan", &planning(&["/repos/x/src/**"]));
    let mut owed = json!({
        "$type": "controls/control/v1",
        "kind": "obligation",
        "requirement": "Sweep the docs.",
        "purpose": "p",
    });
    for (key, value) in reach.as_object().unwrap() {
        owed[key] = value.clone();
    }
    board.write("controls/x/docs.json", &owed.to_string());
}

#[test]
fn review_refuses_an_obligation_on_a_file_the_change_adds_to_its_plans_paths() {
    let board = Changes::new();
    adding_a_file_under_an_obligation(&board, "docs/b.md", &json!({"paths": ["repos/x/docs/**"]}));
    let refused = board.refusal("advance", &path());
    assert_eq!(refused["code"], "changes::gate::unmet");
    assert_eq!(refused["fields"]["missing"], json!(["obligations-settled"]));
    let message = refused["message"].as_str().unwrap();
    assert!(
        message.contains("the plan's paths or to the files the change adds to them"),
        "{message}"
    );
    let unrelated = Changes::new();
    adding_a_file_under_an_obligation(
        &unrelated,
        "docs/b.md",
        &json!({"paths": ["repos/x/other/**"]}),
    );
    assert_eq!(advanced(&unrelated), "land");
}

#[test]
fn review_refuses_an_obligation_naming_an_added_file_whose_name_holds_glob_characters() {
    let board = Changes::new();
    let named = json!({"applies_to": ["/repos/x/app/[id]/page.tsx"]});
    adding_a_file_under_an_obligation(&board, "app/[id]/page.tsx", &named);
    let refused = board.refusal("advance", &path());
    assert_eq!(refused["fields"]["missing"], json!(["obligations-settled"]));
    let other = Changes::new();
    let elsewhere = json!({"applies_to": ["/repos/x/app/i/page.tsx"]});
    adding_a_file_under_an_obligation(&other, "app/[id]/page.tsx", &elsewhere);
    assert_eq!(advanced(&other), "land");
}

#[test]
fn review_finds_no_obligations_while_the_controls_plugin_is_not_enabled() {
    let board = Changes::enabling(&["changes"]);
    reviewing_with_an_obligation(&board);
    assert_eq!(advanced(&board), "land");
}

/// The `obligations` check of a change in plan whose planned paths an
/// obligation at `stored` of `rule_type` reaches, and whose plan owes it
/// under `field` as `owed`, or owes nothing when `field` is empty.
fn obligations_owing(stored: &str, rule_type: &str, field: &str, owed: &str) -> Value {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    board.mount_x(&[("src/a.rs", "a\n")]);
    let rule = json!({
        "$type": rule_type,
        "kind": "obligation",
        "requirement": "Sweep the docs.",
        "purpose": "p",
        "paths": ["repos/x/**"],
    });
    board.write(&format!("{stored}.json"), &rule.to_string());
    let mut plan = planning(&["/repos/x/src/**"]);
    if !field.is_empty() {
        plan["owed"] = json!([{field: owed, "why": "The next step does it."}]);
    }
    board.call_stdin("plan", &plan);
    let next = board.call("next", &path());
    (next["gate"].as_array().unwrap().iter())
        .find(|each| each["check"] == "obligations")
        .unwrap()
        .clone()
}

#[test]
fn a_plan_owing_an_obligation_under_control_gates_as_one_owing_it_under_rule() {
    let now = obligations_owing("rules/x/docs", "rules/rule/v1", "rule", "/rules/x/docs");
    assert_eq!(now["status"], "pass", "{now}");
    // A plan may name a rule under either prefix, wherever it is stored,
    // and the gate names an obligation where it is stored.
    for (stored, rule_type) in [
        ("rules/x/docs", "rules/rule/v1"),
        ("controls/x/docs", "controls/control/v1"),
    ] {
        for field in ["rule", "control"] {
            for owed in ["/rules/x/docs", "/controls/x/docs"] {
                assert_eq!(
                    obligations_owing(stored, rule_type, field, owed),
                    now,
                    "{stored} {field} {owed}"
                );
            }
        }
        let unowed = obligations_owing(stored, rule_type, "", "");
        assert_eq!(unowed["status"], "missing", "{unowed}");
        assert_eq!(
            unowed["detail"],
            format!("these obligations apply to the plan's paths: /{stored}"),
            "{stored}"
        );
    }
}

/// The reason the reopen tests give.
const REASON: &str = "The review asked for a test of the empty case.";
/// The phases of the shipped development flow.
const DEVELOPMENT: [&str; 4] = ["plan", "build", "review", "land"];

/// A board mounting /repos/x with [`CHANGE`] in `phase` and `fields`,
/// migrated to a `changes/change/v2` on the development flow.
fn migrated(phase: &str, fields: &Value) -> Changes {
    let board = Changes::new();
    board.mount_x(&[("README.md", "x\n")]);
    board.change_at("changes/t.json", phase, fields);
    board.call("migrate", &json!({"apply": true}));
    board
}

/// The input of `reopen` for [`CHANGE`] with [`REASON`], by the agent
/// named lead.
fn reopening() -> Value {
    at(&json!({"reason": REASON, "agent": "lead"}))
}

/// The `changes/reopen/v1` records under [`CHANGE`], oldest first.
fn reopens(board: &Changes) -> Vec<Value> {
    let Ok(listed) = std::fs::read_dir(board.root().join("changes/t/reopens")) else {
        return Vec::new();
    };
    let mut names: Vec<String> = (listed.map(|entry| entry.unwrap().file_name()))
        .map(|name| name.into_string().unwrap())
        .collect();
    names.sort();
    (names.iter())
        .map(|name| board.read(&format!("changes/t/reopens/{name}")))
        .collect()
}

/// The change record of [`CHANGE`] without the fields `reopen` changes.
fn unmoved(board: &Changes) -> Value {
    let mut change = board.read("changes/t.json");
    let fields = change.as_object_mut().unwrap();
    fields.remove("phase");
    fields.remove("touched");
    change
}

/// Moves [`CHANGE`] to `phase` by rewriting its record.
fn move_to(board: &Changes, phase: &str) {
    let mut change = board.read("changes/t.json");
    change["phase"] = json!(phase);
    board.write("changes/t.json", &change.to_string());
}

#[test]
fn reopen_moves_a_change_in_review_back_to_build_and_records_why() {
    let board = migrated("review", &json!({}));
    assert_eq!(
        board.call("reopen", &reopening()),
        json!({"path": CHANGE, "from": "review", "to": "build"})
    );
    let change = board.read("changes/t.json");
    assert_eq!(
        (&change["phase"], &change["touched"]),
        (&json!("build"), &json!(today()))
    );
    assert_eq!(
        reopens(&board),
        [json!({
            "$type": "changes/reopen/v1",
            "reason": REASON,
            "from": "review",
            "by": {"agent": "lead"},
        })]
    );
}

#[test]
fn two_reopens_leave_two_records_and_the_changes_other_fields_as_they_were() {
    let board = migrated("review", &json!({"route": "owner"}));
    let before = unmoved(&board);
    board.call("reopen", &reopening());
    move_to(&board, "land");
    let again = at(&json!({"reason": "The land check failed.", "session": "local_abc123"}));
    board.call("reopen", &again);
    assert_eq!(unmoved(&board), before);
    let found: Vec<(Value, Value, Value)> = (reopens(&board).into_iter())
        .map(|record| {
            (
                record["reason"].clone(),
                record["from"].clone(),
                record["by"].clone(),
            )
        })
        .collect();
    assert_eq!(
        found,
        [
            (json!(REASON), json!("review"), json!({"agent": "lead"})),
            (
                json!("The land check failed."),
                json!("land"),
                json!({"session": "local_abc123"})
            ),
        ]
    );
}

#[test]
fn reopen_refuses_a_call_that_names_nobody_or_a_blank_agent_and_admits_its_twin_that_names_one() {
    let board = migrated("review", &json!({}));
    let nobody = at(&json!({"reason": REASON}));
    assert_eq!(
        board.refusal("reopen", &nobody)["code"],
        "board::plugin::bad_input"
    );
    assert_eq!(board.read("changes/t.json")["phase"], "review");
    let blank = at(&json!({"reason": REASON, "agent": "  "}));
    assert_eq!(
        board.refusal("reopen", &blank)["code"],
        "changes::reopen::no_actor"
    );
    assert_eq!(reopens(&board), Vec::<Value>::new());
    let missing = json!({"path": "/changes/missing", "reason": REASON, "agent": "  "});
    assert_eq!(
        board.refusal("reopen", &missing)["code"],
        "changes::change::not_found"
    );
    let named = at(&json!({"reason": REASON, "agent": "lead"}));
    assert_eq!(board.call("reopen", &named)["to"], "build");
    assert_eq!(reopens(&board)[0]["by"], json!({"agent": "lead"}));
}

#[test]
fn reopen_refuses_a_change_already_in_build_or_in_plan_and_admits_one_in_land() {
    let board = migrated("build", &json!({}));
    let refused = board.refusal("reopen", &reopening());
    assert_eq!(refused["code"], "changes::reopen::no_build");
    assert_eq!(
        (&refused["fields"]["phase"], &refused["fields"]["phases"]),
        (&json!("build"), &json!(DEVELOPMENT))
    );
    let message = refused["message"].as_str().unwrap();
    assert!(message.contains("is already in build"), "{message}");
    move_to(&board, "plan");
    assert_eq!(
        board.refusal("reopen", &reopening())["code"],
        "changes::reopen::no_build"
    );
    move_to(&board, "land");
    assert_eq!(
        board.call("reopen", &reopening()),
        json!({"path": CHANGE, "from": "land", "to": "build"})
    );
}

/// A board flow `name` with `phases`, each with no checks.
fn bare_flow(name: &str, phases: &[&str]) -> String {
    let listed: Vec<Value> = (phases.iter())
        .map(|phase| {
            json!({"name": phase, "agent": "lead", "instruction": format!("Do {phase}."), "checks": []})
        })
        .collect();
    json!({
        "$type": "changes/flow/v1",
        "name": name,
        "summary": "A flow for a test.",
        "stop": {"through": phases[0]},
        "phases": listed,
    })
    .to_string()
}

#[test]
fn reopen_refuses_a_change_whose_flow_has_no_build_and_admits_its_twin_on_a_flow_with_one() {
    let board = migrated("review", &json!({}));
    board.write(
        "flows/checked.json",
        &bare_flow("checked", &["plan", "review"]),
    );
    board.call("reflow", &json!({"path": CHANGE, "flow": "checked"}));
    let refused = board.refusal("reopen", &reopening());
    assert_eq!(refused["code"], "changes::reopen::no_build");
    assert_eq!(refused["fields"]["phases"], json!(["plan", "review"]));
    board.write(
        "flows/checked.json",
        &bare_flow("checked", &["plan", "build", "review"]),
    );
    board.call("reflow", &json!({"path": CHANGE, "flow": "checked"}));
    assert_eq!(board.call("reopen", &reopening())["to"], "build");
}

#[test]
fn reopen_refuses_a_change_that_follows_no_flow_and_admits_its_twin_that_follows_one() {
    let board = migrated("review", &json!({}));
    let kept = board.read("changes/t.json");
    let mut flowless = kept.clone();
    flowless.as_object_mut().unwrap().remove("flow");
    board.write("changes/t.json", &flowless.to_string());
    let refused = board.refusal("reopen", &reopening());
    assert_eq!(
        (&refused["code"], &refused["fields"]["phases"]),
        (&json!("changes::reopen::no_build"), &json!([]))
    );
    let message = refused["message"].as_str().unwrap();
    assert!(message.contains("follows no flow"), "{message}");
    board.write("changes/t.json", &kept.to_string());
    assert_eq!(board.call("reopen", &reopening())["to"], "build");
}

#[test]
fn reopen_refuses_a_change_whose_flow_puts_build_after_its_phase_and_admits_one_with_build_before()
{
    let board = migrated("review", &json!({}));
    board.write(
        "flows/late.json",
        &bare_flow("late", &["plan", "review", "build"]),
    );
    board.call("reflow", &json!({"path": CHANGE, "flow": "late"}));
    let refused = board.refusal("reopen", &reopening());
    assert_eq!(
        (&refused["code"], &refused["fields"]["phases"]),
        (
            &json!("changes::reopen::no_build"),
            &json!(["plan", "review", "build"])
        )
    );
    board.write(
        "flows/late.json",
        &bare_flow("late", &["plan", "build", "review"]),
    );
    board.call("reflow", &json!({"path": CHANGE, "flow": "late"}));
    assert_eq!(board.call("reopen", &reopening())["to"], "build");
}

#[test]
fn reopen_refuses_a_done_or_dropped_change_and_admits_one_in_review() {
    let board = migrated("done", &json!({}));
    let refused = board.refusal("reopen", &reopening());
    assert_eq!(
        (&refused["code"], &refused["fields"]["phase"]),
        (&json!("changes::phase::final"), &json!("done"))
    );
    move_to(&board, "dropped");
    assert_eq!(
        board.refusal("reopen", &reopening())["code"],
        "changes::phase::final"
    );
    move_to(&board, "review");
    assert_eq!(board.call("reopen", &reopening())["to"], "build");
}

#[test]
fn reopen_refuses_a_paused_change_and_says_to_resume_it_and_admits_one_in_review() {
    let board = migrated("paused", &json!({}));
    let refused = board.refusal("reopen", &reopening());
    assert_eq!(refused["code"], "changes::phase::paused");
    let message = refused["message"].as_str().unwrap();
    assert!(message.contains("changes resume"), "{message}");
    move_to(&board, "review");
    assert_eq!(board.call("reopen", &reopening())["to"], "build");
}

#[test]
fn reopen_refuses_a_v1_change_until_it_is_migrated() {
    let board = Changes::new();
    board.mount_x(&[("README.md", "x\n")]);
    board.change_at("changes/t.json", "review", &json!({}));
    assert_eq!(
        board.refusal("reopen", &reopening())["code"],
        "changes::change::not_migrated"
    );
    board.call("migrate", &json!({"apply": true}));
    assert_eq!(board.call("reopen", &reopening())["to"], "build");
}

#[test]
fn next_in_build_gives_the_latest_reopens_reason_and_says_nothing_of_one_never_reopened() {
    let board = migrated("build", &json!({}));
    let said = board.call("next", &path())["instruction"].clone();
    assert!(!said.as_str().unwrap().contains("reopened"), "{said}");
    move_to(&board, "review");
    board.call(
        "reopen",
        &at(&json!({"reason": "An earlier round.", "agent": "lead"})),
    );
    move_to(&board, "review");
    board.call("reopen", &reopening());
    let said = board.call("next", &path())["instruction"].clone();
    let said = said.as_str().unwrap();
    assert!(
        said.ends_with(&format!("It was reopened for a fix round: {REASON}")),
        "{said}"
    );
    assert!(!said.contains("An earlier round."), "{said}");
    move_to(&board, "review");
    let said = board.call("next", &path())["instruction"].clone();
    assert!(!said.as_str().unwrap().contains("reopened"), "{said}");
}

#[test]
fn next_names_the_plan_input_where_its_instruction_asks_for_a_plan_and_nowhere_else() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({"route": "owner"}));
    let said = board.call("next", &path())["instruction"].clone();
    assert!(
        said.as_str()
            .unwrap()
            .contains(&new_plan_usage(CHANGE, "/repos/x")),
        "{said}"
    );
    // The development flow's build instruction asks for no plan.
    move_to(&board, "build");
    let said = board.call("next", &path())["instruction"].clone();
    assert!(!said.as_str().unwrap().contains("changes.plan"), "{said}");
}

/// `changes plan-amend` adds the test sections without replacing the plan,
/// so the land instruction that names it carries no whole-plan resend.
#[test]
fn next_at_land_names_plan_amend_and_not_the_whole_plan_resend_that_review_names() {
    let board = Changes::new();
    board.change_at("changes/t.json", "land", &json!({}));
    board.call_stdin("plan", &planning(&["/repos/x/src/**"]));
    let said = board.call("next", &path())["instruction"].clone();
    let said = said.as_str().unwrap();
    assert!(
        said.contains("`board changes plan-amend --path /changes/t`"),
        "{said}"
    );
    assert!(
        !said.contains(&revised_plan_usage(CHANGE, "/repos/x")),
        "{said}"
    );
    board.change_at("changes/t.json", "review", &json!({}));
    let said = board.call("next", &path())["instruction"].clone();
    assert!(
        said.as_str()
            .unwrap()
            .contains(&revised_plan_usage(CHANGE, "/repos/x")),
        "{said}"
    );
}

#[test]
fn next_in_review_says_to_resend_a_plan_without_paths_and_asks_a_change_without_one_for_a_new_one()
{
    let board = Changes::new();
    board.change_at(
        "changes/t.json",
        "review",
        &json!({"plan": "/changes/t/plan.md"}),
    );
    board.write("changes/t/plan.md", "# T\n");
    let said = board.call("next", &path())["instruction"].clone();
    let said = said.as_str().unwrap();
    assert!(
        said.contains(&revised_plan_usage(CHANGE, "/repos/x")),
        "{said}"
    );
    assert!(
        !said.contains(&new_plan_usage(CHANGE, "/repos/x")),
        "{said}"
    );
    board.change_at("changes/u.json", "review", &json!({}));
    let said = board.call("next", &json!({"path": "/changes/u"}))["instruction"].clone();
    let said = said.as_str().unwrap();
    assert!(
        said.contains(&new_plan_usage("/changes/u", "/repos/x")),
        "{said}"
    );
    assert!(
        !said.contains(&revised_plan_usage("/changes/u", "/repos/x")),
        "{said}"
    );
}

#[test]
fn next_in_review_says_to_resend_the_current_plan_since_changes_plan_replaces_it() {
    let board = Changes::new();
    board.change_at("changes/t.json", "review", &json!({}));
    board.call_stdin("plan", &planning(&["/repos/x/src/**"]));
    let said = board.call("next", &path())["instruction"].clone();
    let said = said.as_str().unwrap();
    assert!(
        said.contains(&revised_plan_usage(CHANGE, "/repos/x")),
        "{said}"
    );
    assert!(
        !said.contains(&new_plan_usage(CHANGE, "/repos/x")),
        "{said}"
    );
}

#[test]
fn the_plan_example_for_a_change_on_the_board_itself_names_paths_under_the_root() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({"mount": "/"}));
    let said = board.call("next", &path())["instruction"].clone();
    assert!(
        said.as_str().unwrap().contains(&new_plan_usage(CHANGE, "")),
        "{said}"
    );
}

/// A board whose change in build attaches an open task in the repository
/// mounted at /repos/notes.
fn attaching_a_mounted_task() -> Changes {
    let board = Changes::new();
    let schema = board.text(".board/types/tasks/task/v2.schema.json");
    let open = task("open");
    board.mount_repo(
        &[
            (".board/types/tasks/task/v2.schema.json", &schema),
            ("tasks/t.md", &open),
        ],
        "repos/notes",
        "HEAD",
    );
    board.write("changes/t/plan.md", PLAN);
    board.change_at(
        "changes/t.json",
        "build",
        &json!({"plan": "/changes/t/plan.md", "records": ["/repos/notes/tasks/t.md"]}),
    );
    board
}

#[test]
fn an_open_task_in_a_mount_that_can_be_read_fails_the_tasks_gate() {
    let board = attaching_a_mounted_task();
    let gate = board.call("next", &path())["gate"].clone();
    let tasks = (gate.as_array().unwrap().iter())
        .find(|check| check["check"] == "tasks")
        .unwrap();
    assert_eq!(tasks["status"], "missing");
}

#[test]
fn an_open_task_beneath_a_mount_that_cannot_be_read_refuses_next_and_advance_naming_the_mount() {
    for (losing_git, code) in [
        (true, "board::mount::repo_missing"),
        (false, "board::mount::commit_missing"),
    ] {
        let board = attaching_a_mounted_task();
        board.break_mount("repos/notes", losing_git);
        for capability in ["next", "advance"] {
            let refused = board.refusal(capability, &path());
            assert_eq!(refused["code"], "changes::board::refused", "{refused}");
            assert_eq!(refused["fields"]["code"], code, "{refused}");
            let message = refused["message"].as_str().unwrap();
            assert!(message.contains("/repos/notes"), "{message}");
        }
    }
}

/// `attach` reads the change and its plan, writes the reference and reads
/// the plan back in one call. The fixture board does not commit on put, so
/// HEAD stays where it was and only the put dropping the call's snapshots
/// lets the read-back see the new reference.
#[test]
fn attach_reads_back_each_reference_it_wrote_in_the_same_call() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({"records": []}));
    board.write("tasks/a.md", &task("open"));
    board.write("tasks/b.md", &task("open"));
    let counts: Vec<Value> = (["/tasks/a.md", "/tasks/b.md"].iter())
        .map(|reference| board.call("attach", &at(&json!({"ref": reference})))["records"].clone())
        .collect();
    assert_eq!(counts, [json!(1), json!(2)]);
    assert_eq!(
        board.read("changes/t/plans/1.json")["records"],
        json!(["/tasks/a.md", "/tasks/b.md"])
    );
}

#[test]
fn next_and_advance_from_plan_to_build_read_through_one_view_and_one_report_snapshot() {
    let board = Changes::new();
    board.change_at(
        "changes/t.json",
        "plan",
        &json!({"plan": "/changes/t/plan.md", "route": "owner", "records": [board.approved()]}),
    );
    board.write("changes/t/plan.md", PLAN);
    board.call_stdin("plan", &planning(&["/repos/x/src/**"]));
    let next = board.snapshots("next", &path());
    let advance = board.snapshots("advance", &path());
    assert_eq!(board.read("changes/t.json")["phase"], "build");
    assert_eq!((next, advance), (2, 2));
}

/// Writes the record-paths rule of /repos/x, listing its plan directory.
fn record_paths(board: &Changes) {
    board.write(
        "rules/x/record-paths.json",
        &typed(
            "rules/rule/v1",
            &json!({
                "requirement": "These files are records, not code.",
                "purpose": "p",
                "paths": ["repos/x/plan/*"],
            }),
        ),
    );
}

/// Decision 1318Z through the real index and git: a review of an earlier
/// commit passes once each later commit edits only a path the record-paths
/// rule lists, and a later code commit, or no rule, needs a new review.
#[test]
fn a_review_survives_a_later_commit_that_edits_only_a_record_path_and_not_one_that_edits_code() {
    let board = Changes::new();
    let (base, reviewed) = reviewing(&board, &["/r/1"]);
    board.write("r/1.json", &approving(&reviewed, &base));
    board.call_stdin("plan", &planning(&["/repos/x/src/**", "/repos/x/plan/**"]));
    let noted = board.commit(&[("plan/notes.md", "noted\n")], None);
    board.call("attach", &at(&json!({"head": noted})));
    let review = |board: &Changes| {
        let next = board.call("next", &path());
        (next["gate"].as_array().unwrap().iter())
            .find(|each| each["check"] == "review")
            .unwrap()
            .clone()
    };
    let without = review(&board);
    assert_eq!(without["status"], "missing", "{without}");
    assert!(
        without["detail"]
            .as_str()
            .unwrap()
            .contains("no rule /rules/x/record-paths exists"),
        "{without}"
    );
    record_paths(&board);
    let kept = review(&board);
    assert_eq!(kept["status"], "pass", "{kept}");
    assert!(kept["detail"].as_str().unwrap().contains(&noted), "{kept}");
    let code = board.commit(&[("src/b.rs", "b\n")], None);
    board.call("attach", &at(&json!({"head": code})));
    let lost = review(&board);
    assert_eq!(lost["status"], "missing", "{lost}");
    assert!(
        lost["detail"]
            .as_str()
            .unwrap()
            .contains("/repos/x/src/b.rs"),
        "{lost}"
    );
}

/// The change's lifecycle doc is a record path, whatever file the
/// lifecycle record names, so a later commit to it keeps the review.
#[test]
fn a_review_survives_a_later_commit_to_the_lifecycle_doc() {
    let board = Changes::new();
    let (base, reviewed) = reviewing(&board, &["/r/1"]);
    board.write("r/1.json", &approving(&reviewed, &base));
    record_paths(&board);
    board.write(
        "changes/t/lifecycle.json",
        &typed(
            "changes/lifecycle/v1",
            &json!({"text": "/changes/t/lifecycle.md", "doc": "/repos/x/src/a.rs",
                    "external": []}),
        ),
    );
    board.write("changes/t/lifecycle.md", "| State | Event | Next |\n");
    let doc = board.commit(&[("src/a.rs", "edited\n")], None);
    board.call("attach", &at(&json!({"head": doc})));
    let next = board.call("next", &path());
    let review = (next["gate"].as_array().unwrap().iter())
        .find(|each| each["check"] == "review")
        .unwrap();
    assert_eq!(review["status"], "pass", "{review}");
    assert!(
        review["detail"].as_str().unwrap().contains(&doc),
        "{review}"
    );
}

/// The fix round of decision 20261122T2130Z through the real index and
/// git: a review with no high finding counts after a code commit once an
/// attached changes/fix/v1 names that commit and the findings it answers.
/// A fix record that records a deviation from the plan does not count, and
/// neither does one that answers a review with a high finding.
#[test]
fn a_fix_commit_a_fix_record_answers_keeps_a_review_with_no_high_finding_and_not_one_with_a_high() {
    let board = Changes::new();
    let (base, reviewed) = reviewing(&board, &["/r/1"]);
    let findings = |severity: &str| {
        json!({"findings": [{"severity": severity, "text": "first"},
                            {"severity": "low", "text": "second"}]})
    };
    board.write(
        "r/1.json",
        &report(&reviewed, "approve", &base, &findings("medium")),
    );
    record_paths(&board);
    let fix = board.commit(&[("src/a.rs", "fixed\n")], None);
    board.call("attach", &at(&json!({"head": fix})));
    let review = |board: &Changes| {
        let next = board.call("next", &path());
        (next["gate"].as_array().unwrap().iter())
            .find(|each| each["check"] == "review")
            .unwrap()
            .clone()
    };
    let unrecorded = review(&board);
    assert_eq!(unrecorded["status"], "missing", "{unrecorded}");
    assert!(
        unrecorded["detail"]
            .as_str()
            .unwrap()
            .contains("no changes/fix/v1 names it"),
        "{unrecorded}"
    );
    let fixing = |fields: &Value| {
        typed(
            "changes/fix/v1",
            &serde_json::from_str(&record(
                json!({"report": "/r/1", "commits": [{"commit": fix, "findings": [1, 2]}]}),
                fields,
            ))
            .unwrap(),
        )
    };
    board.write("changes/t/fixes/1.json", &fixing(&json!({})));
    board.call("attach", &at(&json!({"ref": "/changes/t/fixes/1"})));
    let kept = review(&board);
    assert_eq!(kept["status"], "pass", "{kept}");
    assert!(kept["detail"].as_str().unwrap().contains(&fix), "{kept}");

    board.write(
        "changes/t/fixes/1.json",
        &fixing(&json!({"deviation": "the fix also reorders the walk"})),
    );
    let deviated = review(&board);
    assert_eq!(deviated["status"], "missing", "{deviated}");
    assert!(
        deviated["detail"].as_str().unwrap().contains(
            "/changes/t/fixes/1 records a deviation from the plan, so the commits it names need \
             a review of the head"
        ),
        "{deviated}"
    );

    board.write("changes/t/fixes/1.json", &fixing(&json!({})));
    board.write(
        "r/1.json",
        &report(&reviewed, "approve", &base, &findings("high")),
    );
    let high = review(&board);
    assert_eq!(high["status"], "missing", "{high}");
    assert!(
        high["detail"]
            .as_str()
            .unwrap()
            .contains("/changes/t/fixes/1 answers /r/1, which has a high finding"),
        "{high}"
    );
}

/// A fix record that names a finding the report does not have counts for
/// nothing, and the review check says which number.
#[test]
fn a_fix_record_naming_a_finding_the_review_lacks_counts_for_nothing() {
    let board = Changes::new();
    let (base, reviewed) = reviewing(&board, &["/r/1"]);
    board.write(
        "r/1.json",
        &report(
            &reviewed,
            "approve",
            &base,
            &json!({"findings": [{"severity": "medium", "text": "only"}]}),
        ),
    );
    record_paths(&board);
    let fix = board.commit(&[("src/a.rs", "fixed\n")], None);
    board.call("attach", &at(&json!({"head": fix})));
    board.write(
        "changes/t/fixes/1.json",
        &typed(
            "changes/fix/v1",
            &json!({"report": "/r/1", "commits": [{"commit": fix, "findings": [2]}]}),
        ),
    );
    board.call("attach", &at(&json!({"ref": "/changes/t/fixes/1"})));
    let next = board.call("next", &path());
    let review = (next["gate"].as_array().unwrap().iter())
        .find(|each| each["check"] == "review")
        .unwrap();
    assert_eq!(review["status"], "missing", "{review}");
    assert!(
        review["detail"]
            .as_str()
            .unwrap()
            .contains("/changes/t/fixes/1 names finding 2, which /r/1 does not have"),
        "{review}"
    );
}

/// A clone beside the board of the repository /repos/x mounts, its origin
/// that repository, on the branch the repository has checked out.
fn checkout_of_x(board: &Changes) -> PathBuf {
    let from = board.repo_root("repos/x");
    git(
        &board.beside,
        &["clone", "-q", from.to_str().unwrap(), "checkout"],
    );
    board.beside.join("checkout")
}

/// What `next` said it filled.
fn filled(next: &Value) -> Vec<String> {
    (next["filled"].as_array().into_iter().flatten())
        .map(|line| line.as_str().unwrap().to_owned())
        .collect()
}

/// The status of the check `id` in what `next` said.
fn status_of(next: &Value, id: &str) -> Value {
    (next["gate"].as_array().unwrap().iter())
        .find(|each| each["check"] == id)
        .map_or(Value::Null, |each| each["status"].clone())
}

/// /changes/t in build, routed, with a plan whose paths cover the files
/// [`Changes::edited`] and the tests' checkouts commit under `src/`.
fn building_planned(board: &Changes) {
    board.change_at("changes/t.json", "build", &json!({"route": "owner"}));
    board.call_stdin("plan", &planning(&["/repos/x/src/**"]));
}

#[test]
fn next_from_a_feature_branch_checkout_of_the_mount_fills_the_head_and_from_the_default_branch_or_a_branch_with_no_commit_of_its_own_does_not()
 {
    let board = Changes::new();
    let (_, head) = board.edited(&[("src/a.rs", "a\n")]);
    building_planned(&board);
    let checkout = checkout_of_x(&board);
    git(&checkout, &["checkout", "-q", "main"]);
    let next = board.call_from(&checkout, "next", &path());
    assert_eq!(filled(&next), Vec::<String>::new());
    assert_eq!(status_of(&next, "head"), "missing");
    assert_eq!(board.read("changes/t.json").get("head"), None);
    git(&checkout, &["checkout", "-qb", "fresh", "main"]);
    let next = board.call_from(&checkout, "next", &path());
    assert_eq!(filled(&next), Vec::<String>::new());
    assert_eq!(board.read("changes/t.json").get("head"), None);

    git(&checkout, &["checkout", "-q", "work"]);
    let next = board.call_from(&checkout, "next", &path());
    assert_eq!(
        filled(&next),
        [format!(
            "head filled from {} at {head}; set another with `board changes attach --path \
             /changes/t --head <sha>`",
            checkout.display()
        )]
    );
    assert_eq!(status_of(&next, "head"), "pass");
    assert_eq!(board.read("changes/t.json")["head"], json!(head));
}

#[test]
fn next_from_a_directory_that_is_no_checkout_of_the_mount_fills_no_head_and_from_a_checkout_does() {
    let board = Changes::new();
    let (_, head) = board.edited(&[("src/a.rs", "a\n")]);
    building_planned(&board);
    let other = board.beside.join("other");
    make_dir(&other);
    git(&other, &["init", "-q"]);
    write_file(&other.join("a"), "a\n");
    git(&other, &["add", "."]);
    git(&other, &["commit", "-qm", "other"]);
    git(&other, &["checkout", "-qb", "work"]);
    assert_eq!(
        filled(&board.call_from(&other, "next", &path())),
        Vec::<String>::new()
    );
    assert_eq!(board.read("changes/t.json").get("head"), None);
    let checkout = checkout_of_x(&board);
    board.call_from(&checkout, "next", &path());
    assert_eq!(board.read("changes/t.json")["head"], json!(head));
}

#[test]
fn advance_from_a_feature_branch_checkout_fills_the_head_and_names_it() {
    let board = Changes::new();
    let (_, head) = board.edited(&[("src/a.rs", "a\n")]);
    building_planned(&board);
    let checkout = checkout_of_x(&board);
    let target = "changes.advance";
    let (output, _) = board.run_called(
        &checkout,
        &["--json", "call", target, "--path", CHANGE],
        "",
        &[],
    );
    let document = json(&output);
    let fields = &document["fields"];
    assert_eq!(
        fields["filled"],
        json!([format!(
            "head filled from {} at {head}; set another with `board changes attach --path \
             /changes/t --head <sha>`",
            checkout.display()
        )]),
        "{document}"
    );
    assert_eq!(board.read("changes/t.json")["head"], json!(head));
}

#[test]
fn a_change_with_no_route_takes_unowned_and_next_says_so_while_one_with_a_route_keeps_it() {
    let board = Changes::new();
    board.change_at(
        "changes/t.json",
        "plan",
        &json!({"plan": "/changes/t/plan.md", "records": [board.approved()], "route": "owner"}),
    );
    board.write("changes/t/plan.md", PLAN);
    assert_eq!(filled(&board.call("next", &path())), Vec::<String>::new());
    assert_eq!(board.read("changes/t.json")["route"], "owner");

    board.change_at(
        "changes/t.json",
        "plan",
        &json!({"plan": "/changes/t/plan.md", "records": [board.approved()]}),
    );
    let next = board.call("next", &path());
    assert_eq!(
        filled(&next),
        [
            "route filled as unowned; set another with `board changes attach --path /changes/t \
          --route <route>`"
        ]
    );
    assert_eq!(
        (&next["route"], status_of(&next, "route")),
        (&json!("unowned"), json!("pass"))
    );
    assert_eq!(board.read("changes/t.json")["route"], "unowned");
    assert_eq!(advanced(&board), "build");
}

/// Commits that share their first 7 hex digits, made in `repository` on
/// the branch `many`: fast-import writes the same 7,157 commits every
/// run, so the pair is always there.
#[expect(
    clippy::disallowed_methods,
    reason = "fast-import reads its stream on stdin, which the fixture's git does not pass"
)]
fn colliding(repository: &Path) -> (String, String) {
    use std::fmt::Write as _;
    use std::io::Write as _;
    let mut stream = String::new();
    for number in 1..=7157 {
        let message = format!("c{number}\n");
        let from = if number > 1 {
            format!("from :{}\n", number - 1)
        } else {
            String::new()
        };
        write!(
            stream,
            "commit refs/heads/many\nmark :{number}\ncommitter t <t@t> 0 +0000\ndata {}\n\
             {message}{from}deleteall\n\n",
            message.len()
        )
        .unwrap();
    }
    let mut child = std::process::Command::new("git")
        .args(["fast-import", "--quiet"])
        .current_dir(repository)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stream.as_bytes())
        .unwrap();
    assert!(child.wait().unwrap().success());
    let listed = git(repository, &["rev-list", "many"]);
    let mut seen = BTreeMap::new();
    for commit in listed.lines() {
        if let Some(other) = seen.insert(&commit[..7], commit) {
            let mut pair = [other.to_owned(), commit.to_owned()];
            pair.sort();
            let [first, second] = pair;
            return (first, second);
        }
    }
    panic!("the 7,157 commits share no 7-digit prefix");
}

#[test]
fn attach_stores_a_unique_short_head_in_full_and_refuses_an_ambiguous_one_with_the_matches() {
    let board = Changes::new();
    let (_, head) = board.edited(&[("src/a.rs", "a\n")]);
    building_planned(&board);
    board.call("attach", &at(&json!({"head": &head[..7]})));
    assert_eq!(board.read("changes/t.json")["head"], json!(head));

    let (first, second) = colliding(&board.repo_root("repos/x"));
    let refused = board.refusal("attach", &at(&json!({"head": &first[..7]})));
    assert_eq!(
        (&refused["code"], &refused["fields"]["matches"]),
        (
            &json!("changes::attach::head_ambiguous"),
            &json!([first, second])
        ),
        "{refused}"
    );
    assert_eq!(board.read("changes/t.json")["head"], json!(head));
}

#[test]
fn attach_refuses_a_short_head_the_mounted_repository_lacks_and_stores_a_full_one_as_given() {
    let board = Changes::new();
    board.edited(&[("src/a.rs", "a\n")]);
    building_planned(&board);
    let refused = board.refusal("attach", &at(&json!({"head": "eeeeeee"})));
    assert_eq!(
        refused["code"], "changes::attach::head_unknown",
        "{refused}"
    );
    assert_eq!(board.read("changes/t.json").get("head"), None);
    let missing = "e".repeat(40);
    board.call("attach", &at(&json!({"head": missing})));
    assert_eq!(board.read("changes/t.json")["head"], json!(missing));
}

#[test]
fn the_task_remedy_names_the_one_open_task_titled_as_the_change_and_none_when_two_are() {
    let board = Changes::new();
    board.write(
        "changes/t.json",
        &typed(
            "changes/change/v1",
            &json!({"title": "T", "mount": "/repos/x", "phase": "plan", "route": "owner"}),
        ),
    );
    let titled = "---\n$type: tasks/task/v2\ntitle: T\nstatus: open\n---\n";
    board.write("tasks/one.md", titled);
    board.write("tasks/done.md", &titled.replace("open", "done"));
    let remedy = |next: &Value| {
        (next["gate"].as_array().unwrap().iter())
            .find(|each| each["check"] == "task")
            .map(|each| each["remedy"].as_str().unwrap().to_owned())
            .unwrap()
    };
    assert_eq!(
        remedy(&board.call("next", &path())),
        "Set the change's task with `board changes attach --path /changes/t --task /tasks/one.md`."
    );
    let refused = board.refusal("advance", &path());
    assert!(
        (refused["fields"]["remedies"].as_array().unwrap().iter()).any(|each| each
            == "Set the change's task with `board changes attach --path /changes/t --task \
                /tasks/one.md`."),
        "{refused}"
    );
    board.write("tasks/two.md", titled);
    assert!(remedy(&board.call("next", &path())).starts_with("Add a task with `board tasks add`"),);
}

#[test]
fn next_fills_no_head_from_a_checkout_on_another_changes_line_of_work_and_suggests_it() {
    let board = Changes::new();
    let (_, theirs) = board.edited(&[("src/a.rs", "a\n")]);
    board.change_at(
        "changes/a.json",
        "build",
        &json!({"route": "owner", "head": theirs}),
    );
    building_planned(&board);
    let checkout = checkout_of_x(&board);
    // The clone's own `main` judges a commit the mount's clone lacks.
    git(&checkout, &["branch", "-q", "main", "origin/main"]);
    let said = filled(&board.call_from(&checkout, "next", &path()));
    assert_eq!(board.read("changes/t.json").get("head"), None);
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(
        said[0].starts_with(&format!("head not filled: /changes/a records {theirs}")),
        "{said:?}"
    );
    assert!(
        said[0].ends_with(&format!(
            "`board changes attach --path /changes/t --head {theirs}`"
        )),
        "{said:?}"
    );

    write_file(&checkout.join("src/b.rs"), "b\n");
    git(&checkout, &["add", "."]);
    git(&checkout, &["commit", "-qm", "more"]);
    let said = filled(&board.call_from(&checkout, "next", &path()));
    assert_eq!(board.read("changes/t.json").get("head"), None);
    assert!(
        said[0].starts_with(&format!("head not filled: /changes/a records {theirs}")),
        "{said:?}"
    );

    board.change_at(
        "changes/a.json",
        "done",
        &json!({"route": "owner", "head": theirs}),
    );
    board.call_from(&checkout, "next", &path());
    let head = git(&checkout, &["rev-parse", "HEAD"]);
    assert_eq!(board.read("changes/t.json")["head"], json!(head));
}

#[test]
fn next_fills_the_head_only_when_the_plan_has_paths_and_the_branch_touches_one() {
    let board = Changes::new();
    let (_, head) = board.edited(&[("src/a.rs", "a\n")]);
    board.change_at("changes/t.json", "build", &json!({"route": "owner"}));
    let checkout = checkout_of_x(&board);
    let said = filled(&board.call_from(&checkout, "next", &path()));
    assert_eq!(board.read("changes/t.json").get("head"), None);
    assert!(
        said.iter().any(|line| line.starts_with("head not filled: ")
            && line.contains("the change's plan names no paths")),
        "{said:?}"
    );
    board.call_stdin("plan", &planning(&["/repos/x/docs/**"]));
    let said = filled(&board.call_from(&checkout, "next", &path()));
    assert_eq!(board.read("changes/t.json").get("head"), None);
    assert!(
        said.iter().any(|line| line.starts_with("head not filled: ")
            && line.contains("touch none of the plan's paths")),
        "{said:?}"
    );
    board.call_stdin("plan", &planning(&["/repos/x/src/**"]));
    board.call_from(&checkout, "next", &path());
    assert_eq!(board.read("changes/t.json")["head"], json!(head));
}

#[test]
fn next_fills_a_commit_only_a_separate_clone_holds_and_not_a_newer_followed_commit_the_mount_lacks()
{
    let board = Changes::new();
    board.edited(&[("src/a.rs", "a\n")]);
    building_planned(&board);
    let checkout = checkout_of_x(&board);
    git(&checkout, &["checkout", "-q", "main"]);
    write_file(&checkout.join("README.md"), "newer\n");
    git(&checkout, &["commit", "-qam", "newer main"]);
    git(&checkout, &["checkout", "-qb", "fresh"]);
    let said = filled(&board.call_from(&checkout, "next", &path()));
    assert_eq!(said, Vec::<String>::new());
    assert_eq!(board.read("changes/t.json").get("head"), None);

    write_file(&checkout.join("src/b.rs"), "b\n");
    git(&checkout, &["add", "."]);
    git(&checkout, &["commit", "-qm", "mine"]);
    let head = git(&checkout, &["rev-parse", "HEAD"]);
    board.call_from(&checkout, "next", &path());
    assert_eq!(board.read("changes/t.json")["head"], json!(head));
}

#[test]
fn advance_names_what_it_filled_in_a_refusal_that_is_not_the_gates_and_one_that_fills_nothing_does_not()
 {
    let board = Changes::new();
    board.mount_x(&[("README.md", "x\n")]);
    let fields = json!({"plan": "/changes/t/plan.md", "records": [board.approved()]});
    board.change_at("changes/t.json", "plan", &fields);
    board.write("changes/t/plan.md", PLAN);
    board.call("migrate", &json!({"apply": true}));
    let mut change = board.read("changes/t.json");
    change["stop"] = json!({"through": "plan"});
    board.write("changes/t.json", &change.to_string());
    let refused = board.refusal("advance", &path());
    assert_ne!(refused["code"], "changes::gate::unmet", "{refused}");
    assert_eq!(
        refused["fields"]["filled"],
        json!([
            "route filled as unowned; set another with `board changes attach --path \
                /changes/t --route <route>`"
        ]),
        "{refused}"
    );
    let refused = board.refusal("advance", &path());
    assert_eq!(refused["fields"].get("filled"), None, "{refused}");
}

#[test]
fn a_change_with_no_route_takes_the_route_codeowners_gives_and_unowned_when_it_names_nobody() {
    let board = Changes::new();
    board.mount_repo(&[(".github/CODEOWNERS", "* @ana\n")], "repos/x", "main");
    board.change_at("changes/t.json", "plan", &json!({}));
    let next = board.call("next", &path());
    assert_eq!(next["route"], "ask-first", "{next}");
    assert_eq!(
        filled(&next),
        [
            "route filled as ask-first; set another with `board changes attach --path /changes/t \
          --route <route>`"
        ]
    );
    let twin = Changes::new();
    twin.mount_repo(&[(".github/CODEOWNERS", "# nobody\n")], "repos/x", "main");
    twin.change_at("changes/t.json", "plan", &json!({}));
    assert_eq!(twin.call("next", &path())["route"], "unowned");
}

#[test]
fn next_suggests_the_head_while_another_headless_change_plans_paths_its_commits_touch_and_fills_once_none_does()
 {
    let board = Changes::new();
    let (_, head) = board.edited(&[("src/a.rs", "a\n")]);
    board.change_at("changes/a.json", "build", &json!({"route": "owner"}));
    board.call_stdin("plan", &plan_input("/changes/a", &["/repos/x/src/**"]));
    building_planned(&board);
    let checkout = checkout_of_x(&board);
    for (change, other) in [(CHANGE, "/changes/a"), ("/changes/a", CHANGE)] {
        let said = filled(&board.call_from(&checkout, "next", &json!({"path": change})));
        let expected = format!("head not filled: {other}, which records no head, plans paths");
        assert!(
            said.iter().any(|line| line.starts_with(&expected)),
            "{change}: {said:?}"
        );
    }
    assert_eq!(board.read("changes/t.json").get("head"), None);
    assert_eq!(board.read("changes/a.json").get("head"), None);

    board.call_stdin("plan", &plan_input("/changes/a", &["/repos/x/docs/**"]));
    board.call_from(&checkout, "next", &path());
    assert_eq!(board.read("changes/t.json")["head"], json!(head));
}

#[test]
fn a_change_whose_head_is_on_the_followed_branch_blocks_no_fill_and_one_whose_head_builds_on_the_checkout_suggests()
 {
    let board = Changes::new();
    let (base, head) = board.edited(&[("src/a.rs", "a\n")]);
    board.change_at(
        "changes/a.json",
        "land",
        &json!({"route": "owner", "head": base}),
    );
    building_planned(&board);
    let checkout = checkout_of_x(&board);
    board.call_from(&checkout, "next", &path());
    assert_eq!(board.read("changes/t.json")["head"], json!(head));

    building_planned(&board);
    let later = board.commit(&[("src/c.rs", "c\n")], None);
    git(&checkout, &["fetch", "-q", "origin"]);
    board.change_at(
        "changes/a.json",
        "build",
        &json!({"route": "owner", "head": later}),
    );
    let said = filled(&board.call_from(&checkout, "next", &path()));
    assert_eq!(board.read("changes/t.json").get("head"), None);
    assert!(
        said.iter().any(|line| line.starts_with(&format!(
            "head not filled: /changes/a records {later}, on the same line of work as {head}"
        ))),
        "{said:?}"
    );
}

#[test]
fn a_headless_sibling_in_plan_or_paused_that_plans_the_touched_paths_does_not_block_the_fill() {
    let board = Changes::new();
    let (_, head) = board.edited(&[("src/a.rs", "a\n")]);
    for (name, phase) in [("a", "plan"), ("p", "paused")] {
        board.change_at(
            &format!("changes/{name}.json"),
            phase,
            &json!({"route": "owner"}),
        );
        let sibling = format!("/changes/{name}");
        board.call_stdin("plan", &plan_input(&sibling, &["/repos/x/src/**"]));
    }
    building_planned(&board);
    let checkout = checkout_of_x(&board);
    let said = filled(&board.call_from(&checkout, "next", &path()));
    assert_eq!(
        board.read("changes/t.json")["head"],
        json!(head),
        "{said:?}"
    );
}

#[test]
fn next_suggests_the_head_from_a_branch_named_for_another_open_change_and_fills_from_another_branch()
 {
    let board = Changes::new();
    let (_, head) = board.edited(&[("src/a.rs", "a\n")]);
    board.change_at("changes/a.json", "plan", &json!({"route": "owner"}));
    building_planned(&board);
    let checkout = checkout_of_x(&board);
    git(&checkout, &["checkout", "-qb", "a"]);
    let said = filled(&board.call_from(&checkout, "next", &path()));
    assert_eq!(board.read("changes/t.json").get("head"), None);
    assert!(
        said.iter().any(|line| line
            .starts_with("head not filled: the checkout's branch a has the name of /changes/a")),
        "{said:?}"
    );
    git(&checkout, &["checkout", "-qb", "t"]);
    board.call_from(&checkout, "next", &path());
    assert_eq!(board.read("changes/t.json")["head"], json!(head));
}

// `next --through <phase>`: the checks of each later phase, judged ahead
// where the plan or the change record already settles them.

/// A plan-phase /changes/t whose plan file holds `text`.
fn planned_with(text: &str) -> Changes {
    let board = Changes::new();
    board.change_at(
        "changes/t.json",
        "plan",
        &json!({"plan": "/changes/t/plan.md", "route": "owner", "records": [board.approved()]}),
    );
    board.write("changes/t/plan.md", text);
    board
}

/// The check `id` that `next --through land` lists ahead for `board`.
fn ahead_check(board: &Changes, id: &str) -> Value {
    let next = board.call("next", &at(&json!({"through": "land"})));
    (next["ahead"].as_array().unwrap().iter())
        .flat_map(|phase| phase["checks"].as_array().unwrap().iter())
        .find(|each| each["check"] == id)
        .unwrap_or_else(|| panic!("no {id} ahead: {next}"))
        .clone()
}

#[test]
fn next_through_land_lists_each_later_phase_and_judges_what_the_plan_settles() {
    let board = planned_with(PLAN);
    let next = board.call("next", &at(&json!({"through": "land"})));
    let phases: Vec<&str> = (next["ahead"].as_array().unwrap().iter())
        .map(|phase| phase["phase"].as_str().unwrap())
        .collect();
    assert_eq!(phases, ["build", "review", "land"], "{next}");
    let tests = ahead_check(&board, "test-sections");
    assert_eq!(
        (&tests["status"], &tests["blocks"], &tests["applies"]),
        (&json!("missing"), &json!(true), &json!(true)),
        "{tests}"
    );
    let remedy = tests["remedy"].as_str().unwrap();
    assert!(
        remedy.contains("board changes plan-amend --path /changes/t"),
        "{remedy}"
    );
    let run = ahead_check(&board, "suite-run");
    assert_eq!(run["status"], "later", "{run}");
    // The head is not known yet, so the remedy names it as a placeholder.
    let remedy = run["remedy"].as_str().unwrap();
    assert!(remedy.contains("once CI has passed on <head>"), "{remedy}");
    let review = ahead_check(&board, "review");
    let remedy = review["remedy"].as_str().unwrap();
    assert!(remedy.contains("a checkout of <head>,"), "{remedy}");
    // A head, a pull request and a diff come only from later work, so the
    // checks that read them wait for it.
    for later in [
        "head",
        "pull-request",
        "ui-paths",
        "lifecycle-paths-at-land",
    ] {
        let each = ahead_check(&board, later);
        assert_eq!(each["status"], "later", "{each}");
    }
    // The plan answers ux none, so the design-conformance check does not
    // apply and is not judged.
    let design = ahead_check(&board, "design-conformance");
    assert_eq!(design["applies"], false, "{design}");
    assert!(design.get("status").is_none(), "{design}");

    let board = planned_with(&format!(
        "{PLAN}\n## What the tests prove\n\nA suite stops.\n\n## What the tests do not prove\n\n\
         That it stops in time.\n"
    ));
    let tests = ahead_check(&board, "test-sections");
    assert_eq!(tests["status"], "pass", "{tests}");
    assert!(tests.get("remedy").is_none(), "{tests}");
}

#[test]
fn next_without_through_prints_what_it_printed_before() {
    let board = planned_with(PLAN);
    let plain = board.call("next", &path());
    assert!(plain.get("ahead").is_none(), "{plain}");
    let mut through = board.call("next", &at(&json!({"through": "land"})));
    through.as_object_mut().unwrap().remove("ahead");
    assert_eq!(
        serde_json::to_string(&plain).unwrap(),
        serde_json::to_string(&through).unwrap()
    );
}

#[test]
fn next_through_a_phase_the_flow_lacks_is_refused_naming_the_phases() {
    let board = planned_with(PLAN);
    let refused = board.refusal("next", &at(&json!({"through": "nowhere"})));
    assert_eq!(refused["code"], "changes::next::bad_through", "{refused}");
    let message = refused["message"].as_str().unwrap();
    assert!(
        message.contains("plan, build, review and land"),
        "{message}"
    );
    assert_eq!(
        refused["fields"]["phases"],
        json!(["plan", "build", "review", "land"]),
        "{refused}"
    );
    // Through the change's own phase there is nothing ahead.
    let next = board.call("next", &at(&json!({"through": "plan"})));
    assert_eq!(next["ahead"], json!([]), "{next}");
}

#[test]
fn next_through_land_marks_a_later_check_missing_when_its_when_reads_an_answer_the_plan_lacks() {
    let board = planned_with(PLAN);
    let mut plan = board.read("changes/t/plans/1.json");
    plan["answers"] = json!({"ux": "none", "lifecycle": "none"});
    board.write("changes/t/plans/1.json", &plan.to_string());
    let rechecks = ahead_check(&board, "rechecks");
    assert_eq!(
        (&rechecks["status"], &rechecks["applies"]),
        (&json!("missing"), &json!(true)),
        "{rechecks}"
    );
    let detail = rechecks["detail"].as_str().unwrap();
    assert!(detail.contains("no typed answer for data"), "{detail}");
    let remedy = rechecks["remedy"].as_str().unwrap();
    assert!(
        remedy.starts_with("Write the plan again with `board changes plan --path /changes/t`"),
        "{remedy}"
    );
}

/// `advance` on a change already done, as an agent sends it again after a
/// timeout, succeeds with `already_done` and the merge commit, reads no
/// pull request and writes nothing; one done with no merge commit says so
/// without one. The twin is a dropped change, which is refused as final.
#[test]
fn advance_on_a_done_change_succeeds_with_already_done_and_on_a_dropped_change_is_refused() {
    let board = landing(&json!({"pull_request": PULL}));
    board.answers(
        1,
        &json!({"state": "closed", "merged": true, "merge_commit_sha": MERGED}),
    );
    assert_eq!(advanced(&board), "done");
    let stored = board.read("changes/t.json");
    let again = board.call("advance", &path());
    assert_eq!(
        again,
        json!({
            "outcome": "already_done",
            "path": CHANGE,
            "from": "done",
            "to": "done",
            "merge_commit_url": format!("https://github.com/o/r/commit/{MERGED}"),
            "line": "already done: /changes/t is done (merged as 01234567)",
        })
    );
    assert_eq!(
        board.read("changes/t.json"),
        stored,
        "a repeat writes nothing"
    );
    assert_eq!(board.gh_calls(), ["api repos/o/r/pulls/1"]);
    let board = migrated("done", &json!({}));
    let again = board.call("advance", &path());
    assert_eq!(
        (
            &again["outcome"],
            &again["line"],
            again.get("merge_commit_url")
        ),
        (
            &json!("already_done"),
            &json!("already done: /changes/t is done"),
            None
        )
    );
    move_to(&board, "dropped");
    let refused = board.refusal("advance", &path());
    assert_eq!(
        (&refused["code"], &refused["fields"]["phase"]),
        (&json!("changes::phase::final"), &json!("dropped"))
    );
}
