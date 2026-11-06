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
        "The gate passes: run `board call changes.advance --path /changes/t`."
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
        json!(["plan-sections", "owner-approval", "mockups", "data-signed"])
    );
    let remedies = refused["fields"]["remedies"].as_array().unwrap();
    assert_eq!(remedies.len(), 4);
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
    assert!(message.contains("`board call changes.open`"), "{message}");
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
        message.contains("changes.plan --path /changes/t"),
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
    format!("{before}## UX\n\n{ux}\n\n## Data\n\n{data}\n")
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
