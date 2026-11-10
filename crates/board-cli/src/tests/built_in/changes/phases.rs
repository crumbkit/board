//! `test_phases.py`: `next`, `attach` and `advance` over a real board,
//! which read the gate from a change's references and move its phase.

use super::*;

const CHANGE: &str = "/changes/t";
const MOCKUP: &str = "https://claude.ai/artifact/m1";

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

/// What a refusal of `advance` says is missing.
fn missing(board: &Changes) -> Value {
    board.refusal("advance", &path())["fields"]["missing"].clone()
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
            ("owner-approval", "pass"),
            ("mockups", "missing"),
            ("data-signed", "missing"),
            ("lifecycle-record", "missing"),
            ("obligations", "pass"),
        ])
    );
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
    board.change_at(
        "changes/t.json",
        "plan",
        &json!({"plan": "/changes/t/plan.md", "route": "owner"}),
    );
    board.write("changes/t/plan.md", "# T\n");
    let refused = board.refusal("advance", &path());
    assert_eq!(refused["code"], "changes::gate::unmet");
    assert_eq!(
        refused["fields"]["missing"],
        json!([
            "plan-sections",
            "owner-approval",
            "mockups",
            "data-signed",
            "lifecycle-record"
        ])
    );
    let remedies = refused["fields"]["remedies"].as_array().unwrap();
    assert_eq!(remedies.len(), 5);
    assert!(
        remedies[1]
            .as_str()
            .unwrap()
            .starts_with("Only the owner can provide this"),
        "{remedies:?}"
    );
    board.change_at(
        "changes/t.json",
        "plan",
        &json!({"plan": "/changes/t/plan.md", "route": "owner", "records": [board.approved()]}),
    );
    assert_eq!(board.read("changes/t.json")["phase"], "plan");
    board.write("changes/t/plan.md", PLAN);
    assert_eq!(
        board.call("advance", &path()),
        json!({"path": CHANGE, "from": "plan", "to": "build"})
    );
    assert_eq!(board.read("changes/t.json")["phase"], "build");
}

#[test]
fn a_plan_with_no_route_blocks_the_plan_gate_and_setting_one_clears_it() {
    let board = Changes::new();
    board.change_at(
        "changes/t.json",
        "plan",
        &json!({"plan": "/changes/t/plan.md", "records": [board.approved()]}),
    );
    board.write("changes/t/plan.md", PLAN);
    assert_eq!(missing(&board), json!(["route"]));
    board.call("attach", &at(&json!({"route": "unowned"})));
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
fn review_refuses_a_diff_that_edits_a_file_outside_the_planned_paths_until_the_plan_covers_it() {
    let board = Changes::new();
    let (base, head) = board.edited(&[("src/a.rs", "a\n"), ("docs/b.md", "b\n")]);
    board.change_at(
        "changes/t.json",
        "review",
        &json!({"head": head, "records": ["/r/1"]}),
    );
    board.write("r/1.json", &approving(&head, &base));
    board.call_stdin("plan", &planning(&["/repos/x/src/**"]));
    let refused = board.refusal("advance", &path());
    assert_eq!(refused["code"], "changes::paths::outside_plan");
    let message = refused["message"].as_str().unwrap();
    assert!(message.contains("/repos/x/docs/b.md"), "{message}");
    assert!(!message.contains("/repos/x/src/a.rs"), "{message}");
    board.call_stdin(
        "plan",
        &planning(&["/repos/x/src/**", "/repos/x/docs/b.md"]),
    );
    assert_eq!(advanced(&board), "land");
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
    let refused = board.refusal("advance", &path());
    assert_eq!(refused["code"], "changes::paths::outside_plan");
    let message = refused["message"].as_str().unwrap();
    assert!(message.contains("/repos/x/docs/b.md"), "{message}");
    board.call_stdin("plan", &planning(&["/repos/x/src/**", "/repos/x/docs/**"]));
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
    let refused = board.refusal("advance", &path());
    assert_eq!(refused["code"], "changes::paths::outside_plan");
    let message = refused["message"].as_str().unwrap();
    assert!(message.contains("/repos/x/docs/mine.md"), "{message}");
    assert!(!message.contains("/repos/x/docs/upstream.md"), "{message}");
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

#[test]
fn advance_runs_from_land_to_done_and_refuses_to_leave_done() {
    let board = Changes::new();
    board.change_at(
        "changes/t.json",
        "land",
        &json!({"pull_request": "https://github.com/o/r/pull/1", "plan": "/changes/t/plan.md"}),
    );
    board.write("changes/t/plan.md", "# T\n");
    assert_eq!(missing(&board), json!(["test-sections"]));
    board.write(
        "changes/t/plan.md",
        "# T\n\n## What the tests prove\n\nIt lands.\n\n## What the tests do not prove\n\nRetries.\n",
    );
    assert_eq!(advanced(&board), "done");
    let refused = board.refusal("advance", &path());
    assert_eq!(
        (&refused["code"], &refused["fields"]["phase"]),
        (&json!("changes::phase::final"), &json!("done"))
    );
    let message = refused["message"].as_str().unwrap();
    assert!(message.contains("`board changes open`"), "{message}");
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
            &json!({"text": "/changes/t/plan.md", "uses": [], "records": [board.approved()]}),
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
fn ux_plan(ux: &str, data: &str) -> String {
    let before = PLAN.split("## UX").next().unwrap();
    let (_, after) = PLAN.split_once("## Failure paths").unwrap();
    format!("{before}## UX\n\n{ux}\n\n## Data\n\n{data}\n\n## Failure paths{after}")
}

/// An owner-routed change in plan whose plan file holds `plan`.
fn planned_change(board: &Changes, plan: &str) {
    board.change_at(
        "changes/t.json",
        "plan",
        &json!({"plan": "/changes/t/plan.md", "route": "owner", "records": [board.approved()]}),
    );
    board.write("changes/t/plan.md", plan);
}

/// A decision with these refs or urls.
fn decision(fields: &Value) -> String {
    let body = json!({
        "$type": "plans/decision/v1",
        "decision": "Agreed.",
        "why": "Seen.",
        "session": "s",
        "decided": "2026-11-01T10:00:00Z",
    });
    record(body, fields)
}

#[test]
fn advance_refuses_a_mockup_the_owner_has_not_agreed_and_moves_on_once_a_decision_names_it() {
    let board = Changes::new();
    planned_change(
        &board,
        &ux_plan(
            &format!("### Mockups\n\n- {MOCKUP}\n\n### Neighbours\n\n- /changes/other\n"),
            "None.",
        ),
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
fn advance_refuses_mockups_with_no_neighbours_list_and_moves_on_once_one_is_written() {
    let board = Changes::new();
    board.write("decisions/ux.json", &decision(&json!({"urls": [MOCKUP]})));
    planned_change(
        &board,
        &ux_plan(&format!("### Mockups\n\n- {MOCKUP}"), "None."),
    );
    board.call("attach", &at(&json!({"ref": "/decisions/ux"})));
    assert_eq!(
        board.refusal("advance", &path())["code"],
        "changes::ux::no_neighbours"
    );
    board.write(
        "changes/t/plan.md",
        &ux_plan(
            &format!("### Mockups\n\n- {MOCKUP}\n\n### Neighbours\n\n- /changes/other\n"),
            "None.",
        ),
    );
    assert_eq!(advanced(&board), "build");
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

#[test]
fn review_finds_no_obligations_while_the_controls_plugin_is_not_enabled() {
    let board = Changes::enabling(&["changes"]);
    reviewing_with_an_obligation(&board);
    assert_eq!(advanced(&board), "land");
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
