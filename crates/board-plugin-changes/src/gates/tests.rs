//! Each phase's gate in the shipped development flow, as a pure function
//! of the change and the records it references. These were the Python
//! plugin's `test_gates.py`; the check ids follow the flow.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use super::{Observed, Read, Status, gate, mockups, reads, sections};
use crate::change::Referenced;
use crate::flow::development;
use crate::records::Kind;

const PATH: &str = "/changes/t";
const SUITE: &str = "/suites/checks";
const RUN: &str = "/suites/checks/runs/1";
const HEAD: &str = "a3f9c0de1234567890abcdef1234567890abcdef";
const BASE: &str = "0123456789abcdef0123456789abcdef01234567";
const TASK: &str = "/tasks/given.md";
const PLAN: &str = "# Timeout\n\n## Problem\n\nSuites hang.\n\n## Outcome\n\nA suite stops.\n\n## Out of scope\n\nRetries.\n\n## UX\n\nNo UI change: suites run headless.\n\n## Data\n\nNone.\n\n## Failure paths\n\nNone: nothing can fail.\n\n## Consumers\n\nNone: nothing reads it.\n\n## Lifecycle\n\nNone: nothing changes state.\n";
const MOCKUP: &str = "https://claude.ai/artifact/m1";
const NEIGHBOURS: &str = "### Neighbours\n\n- /changes/flows-list, the same area.\n";
const UNDERSTOOD: &str = "/changes/t/understanding/data";
const TESTS: &str =
    "\n## What the tests prove\n\nIt stops.\n\n## What the tests do not prove\n\nRetries.\n";

type Records = Vec<(&'static str, (&'static str, Value))>;

/// What a gate reads for a change in `phase` with a plan at
/// /changes/t/plans/1, the fixture plan and suite, every file planned, and
/// `fields` set on the change.
struct Given {
    phase: &'static str,
    records: Records,
    plan: Option<String>,
    suite: Option<&'static str>,
    planned: Vec<&'static str>,
    outside: Vec<&'static str>,
    diff_unread: Option<&'static str>,
    fields: Value,
    /// Referenced records Board does not read: path, type and code.
    unreadable: Vec<(&'static str, &'static str, &'static str)>,
    /// Whether the newest pull request reads as a github/pull/v1 record.
    pull_read: bool,
    /// The controls `controls.for-paths` finds on the plan's paths.
    on_planned: Vec<&'static str>,
    /// The controls the board holds.
    held: Vec<&'static str>,
    /// What is at each path an `exists` check names.
    existing: Vec<(&'static str, bool)>,
    /// The paths a `when.absent` names that hold the record it means.
    records_held: Vec<&'static str>,
    /// The files a controlled-paths control lists among the edited ones.
    controlled: Vec<(&'static str, Vec<&'static str>)>,
}

fn given(phase: &'static str) -> Given {
    Given {
        phase,
        records: Vec::new(),
        plan: Some(PLAN.to_owned()),
        suite: Some(SUITE),
        planned: vec!["/repos/x/**"],
        outside: Vec::new(),
        diff_unread: None,
        fields: json!({}),
        unreadable: Vec::new(),
        pull_read: true,
        on_planned: Vec::new(),
        held: Vec::new(),
        existing: Vec::new(),
        records_held: Vec::new(),
        controlled: Vec::new(),
    }
}

impl Given {
    fn records(mut self, records: Records) -> Self {
        self.records = records;
        self
    }

    fn plan(mut self, plan: Option<&str>) -> Self {
        self.plan = plan.map(str::to_owned);
        self
    }

    fn with(mut self, fields: Value) -> Self {
        self.fields = fields;
        self
    }

    fn unreadable(mut self, unreadable: Vec<(&'static str, &'static str, &'static str)>) -> Self {
        self.unreadable = unreadable;
        self
    }

    fn observed(&self) -> Observed {
        let mut change = json!({
            "$type": "changes/change/v1",
            "title": "T",
            "mount": "/repos/x",
            "phase": self.phase,
            "task": TASK,
            "plan": "/changes/t/plans/1",
        });
        for (key, value) in self.fields.as_object().unwrap() {
            change[key] = value.clone();
        }
        let change = crate::change::from_value(change).unwrap();
        let records = Referenced {
            records: (self.records.iter())
                .map(|(path, (found, body))| {
                    let body: Map<String, Value> = body.as_object().unwrap().clone();
                    ((*path).to_owned(), ((*found).to_owned(), body))
                })
                .collect(),
            unreadable: (self.unreadable.iter())
                .map(|(path, _, code)| ((*path).to_owned(), (*code).to_owned()))
                .collect(),
            unreadable_types: (self.unreadable.iter())
                .map(|(path, kind, _)| ((*path).to_owned(), (*kind).to_owned()))
                .collect(),
        };
        let mut observed = Observed::new(PATH, change, Kind::Build, records);
        observed.flow = Some(development());
        observed.plan.clone_from(&self.plan);
        observed.plan_file = self
            .plan
            .as_ref()
            .map(|_| "/changes/t/plans/1.md".to_owned());
        observed.suite = self.suite.map(str::to_owned);
        observed.planned = self.planned.iter().map(|each| (*each).to_owned()).collect();
        observed.outside = self.outside.iter().map(|each| (*each).to_owned()).collect();
        observed.diff_unread = self.diff_unread.map(str::to_owned);
        observed.pull_read = self.pull_read;
        observed.on_planned = self
            .on_planned
            .iter()
            .map(|each| (*each).to_owned())
            .collect();
        observed.held = self.held.iter().map(|each| (*each).to_owned()).collect();
        observed.existing = (self.existing.iter())
            .map(|(path, held)| ((*path).to_owned(), *held))
            .collect();
        observed.records_held = (self.records_held.iter())
            .map(|path| (*path).to_owned())
            .collect();
        observed.controlled = (self.controlled.iter())
            .map(|(control, files)| {
                let files = files.iter().map(|each| (*each).to_owned()).collect();
                ((*control).to_owned(), Some(files))
            })
            .collect();
        observed
    }

    /// The remedy the gate gives for the check `id`.
    fn remedy(&self, id: &str) -> String {
        (gate(&self.observed()).into_iter())
            .find(|found| found.check == id)
            .and_then(|found| found.remedy)
            .unwrap()
    }

    /// Each check's status, by name.
    fn statuses(&self) -> BTreeMap<String, &'static str> {
        (gate(&self.observed()).into_iter())
            .map(|found| {
                let status = if found.status == Status::Pass {
                    "pass"
                } else {
                    "missing"
                };
                (found.check, status)
            })
            .collect()
    }

    /// The code `advance` would refuse with for the check `name`.
    fn code(&self, name: &str) -> &'static str {
        (gate(&self.observed()).into_iter())
            .find(|found| found.check == name)
            .unwrap()
            .code
    }

    /// The status and detail of the check `name`.
    fn check(&self, name: &str) -> (&'static str, String) {
        let found = (gate(&self.observed()).into_iter())
            .find(|found| found.check == name)
            .unwrap();
        let status = if found.status == Status::Pass {
            "pass"
        } else {
            "missing"
        };
        (status, found.detail)
    }
}

fn statuses(pairs: &[(&str, &'static str)]) -> BTreeMap<String, &'static str> {
    (pairs.iter())
        .map(|(name, status)| ((*name).to_owned(), *status))
        .collect()
}

/// A decision the owner signed whose refs name the change's plan.
fn approval() -> (&'static str, (&'static str, Value)) {
    (
        "/d/plan",
        ("plans/decision/v1", json!({"refs": ["/changes/t/plans/1"]})),
    )
}

/// A full run of a clean tree of the check suite on the head commit, which
/// passed, with `fields` over it.
fn run(fields: &Value) -> (&'static str, Value) {
    let mut body =
        json!({"scope": "full", "dirty": false, "versions": {"repo": &HEAD[..7]}, "results": []});
    for (key, value) in fields.as_object().unwrap() {
        body[key] = value.clone();
    }
    ("runs/run/v1", body)
}

/// A review report of `base..head` that comments, with one medium finding.
fn report(fields: &Value) -> (&'static str, Value) {
    let mut body = json!({
        "range": format!("{BASE}..{HEAD}"),
        "patch_ids": ["p1"],
        "findings": [{"severity": "medium", "text": "x"}],
        "verdict": "comment",
    });
    for (key, value) in fields.as_object().unwrap() {
        body[key] = value.clone();
    }
    ("review/report/v1", body)
}

#[test]
fn the_plan_gate_passes_when_each_plan_section_has_text_and_misses_when_one_is_empty() {
    let owned = (given("plan").with(json!({"route": "owner"}))).records(vec![approval()]);
    assert!(owned.statuses().values().all(|status| *status == "pass"));
    let thin = "# T\n\n## Problem\n\nX.\n\n## Outcome\n\n## out of scope\nY.\n";
    let owned = owned.plan(Some(thin));
    assert_eq!(
        owned.check("plan-sections"),
        (
            "missing",
            "the plan at /changes/t/plans/1 has no text under: Outcome, UX, Data, Failure paths, Consumers, Lifecycle".to_owned()
        )
    );
}

#[test]
fn the_plan_gate_misses_a_plan_that_is_not_there_and_passes_one_with_text() {
    let owned = || given("plan").with(json!({"route": "owner"}));
    assert_eq!(
        owned().plan(None).check("plan-sections"),
        (
            "missing",
            "no plan record and its text are at /changes/t/plans/1".to_owned()
        )
    );
    assert_eq!(owned().statuses()["plan-sections"], "pass");
}

#[test]
fn the_plan_gate_misses_a_change_with_no_route_and_passes_each_route() {
    assert_eq!(
        given("plan").check("route"),
        (
            "missing",
            "the change records no route; set one with `board changes attach --path \
             /changes/t --route <route>`"
                .to_owned()
        )
    );
    for named in ["owner", "ask-first", "unowned"] {
        let routed = given("plan").with(json!({"route": named}));
        assert_eq!(routed.statuses()["route"], "pass", "{named}");
    }
}

#[test]
fn on_every_route_the_plan_gate_needs_a_signed_decision_whose_refs_name_the_plan_or_the_change() {
    for route in ["owner", "ask-first", "unowned"] {
        let asking = || given("plan").with(json!({"route": route}));
        let unrelated = vec![(
            "/d",
            ("plans/decision/v1", json!({"refs": ["/changes/other"]})),
        )];
        assert_eq!(
            asking().records(unrelated).statuses()["owner-approval"],
            "missing"
        );
        let stub = vec![(
            "/d",
            (
                "plans/decision/v1",
                json!({"refs": [PATH], "written_by": "hook"}),
            ),
        )];
        assert_eq!(
            asking().records(stub).statuses()["owner-approval"],
            "missing"
        );
        for named in ["/changes/t/plans/1", "/changes/t/plans/1.md", PATH] {
            let approved = vec![("/d", ("plans/decision/v1", json!({"refs": [named]})))];
            assert_eq!(
                asking().records(approved).statuses()["owner-approval"],
                "pass",
                "{route} {named}"
            );
        }
    }
}

#[test]
fn the_build_gate_passes_with_tasks_done_or_dropped_and_a_passing_run_on_the_head() {
    let records = vec![
        ("/t/a.md", ("tasks/task/v2", json!({"status": "done"}))),
        ("/t/b.md", ("tasks/task/v2", json!({"status": "dropped"}))),
        (RUN, run(&json!({}))),
    ];
    let found = given("build").records(records).with(json!({"head": HEAD}));
    assert_eq!(
        found.statuses(),
        statuses(&[("head", "pass"), ("suite-run", "pass"), ("tasks", "pass")])
    );
}

#[test]
fn the_build_gate_misses_each_task_still_open() {
    let records = vec![
        ("/t/a.md", ("tasks/task/v2", json!({"status": "open"}))),
        ("/t/b.md", ("tasks/task/v2", json!({"status": "deferred"}))),
        (RUN, run(&json!({}))),
    ];
    let found = given("build").records(records).with(json!({"head": HEAD}));
    assert_eq!(
        found.check("tasks").1,
        "these tasks are not done or dropped: /t/a.md, /t/b.md"
    );
    let own = vec![
        (TASK, ("tasks/task/v2", json!({"status": "open"}))),
        (RUN, run(&json!({}))),
    ];
    let found = given("build").records(own).with(json!({"head": HEAD}));
    assert_eq!(found.statuses()["tasks"], "pass");
}

#[test]
fn the_build_gate_misses_a_run_that_failed_or_is_dirty_targeted_of_another_suite_or_another_commit()
{
    for wrong in [
        json!({"results": [{"case": "a", "outcome": "fail", "note": "x"}]}),
        json!({"results": [{"case": "a", "outcome": "error", "note": "x"}]}),
        json!({"dirty": true}),
        json!({"dirty": null}),
        json!({"scope": "targeted"}),
        json!({"versions": {"repo": "b4d"}}),
        json!({"versions": {"repo": "0000000"}}),
    ] {
        let found = (given("build").records(vec![(RUN, run(&wrong))])).with(json!({"head": HEAD}));
        assert_eq!(found.statuses()["suite-run"], "missing", "{wrong}");
    }
    let skipped = json!({"results": [{"case": "a", "outcome": "skipped", "note": "x"}]});
    let found = given("build")
        .records(vec![(RUN, run(&skipped))])
        .with(json!({"head": HEAD}));
    assert_eq!(found.statuses()["suite-run"], "pass");
}

#[test]
fn a_run_belongs_to_the_suite_whose_runs_directory_holds_it_whatever_its_suite_field_says() {
    for elsewhere in [
        "/suites/other/runs/1",
        "/suites/checks-2/runs/1",
        "/suites/checks/1",
        "/runs/1",
    ] {
        let found = (given("build").records(vec![(elsewhere, run(&json!({"suite": SUITE})))]))
            .with(json!({"head": HEAD}));
        assert_eq!(found.statuses()["suite-run"], "missing", "{elsewhere}");
    }
    let found = (given("build").records(vec![(RUN, run(&json!({"suite": "/suites/other"})))]))
        .with(json!({"head": HEAD}));
    assert_eq!(found.statuses()["suite-run"], "pass");
}

#[test]
fn the_build_gate_says_when_no_suite_is_named_or_no_head_is_recorded() {
    let mut no_suite = given("build")
        .records(vec![(RUN, run(&json!({})))])
        .with(json!({"head": HEAD}));
    no_suite.suite = None;
    assert_eq!(
        no_suite.check("suite-run").1,
        "no contributing doc in /repos/x names a runs/suite/v1 path as its check suite"
    );
    let no_head = given("build").records(vec![(RUN, run(&json!({})))]);
    assert_eq!(
        no_head.check("suite-run").1,
        "the change records no head commit; attach one with --head"
    );
}

/// The review gate's checks for the fixture plan, whose UX and Data say
/// there is nothing to show or store, with the review's status.
fn review_statuses(review: &'static str) -> BTreeMap<String, &'static str> {
    statuses(&[
        ("review", review),
        ("paths", "pass"),
        ("ui-paths", "pass"),
        ("data-paths", "pass"),
        ("lifecycle-paths", "pass"),
        ("lifecycle-none-paths", "pass"),
        ("obligations-settled", "pass"),
    ])
}

#[test]
fn the_review_gate_passes_a_report_on_the_head_without_a_blocking_finding() {
    let found = (given("review").records(vec![("/r", report(&json!({})))]))
        .with(json!({"head": &HEAD[..12]}));
    assert_eq!(found.statuses(), review_statuses("pass"));
    for wrong in [
        json!({"range": format!("{BASE}..{}", "b".repeat(40))}),
        json!({"range": HEAD}),
        json!({"verdict": "changes"}),
        json!({"findings": [{"severity": "high", "text": "x"}]}),
        json!({"findings": [{"severity": "critical", "text": "x"}]}),
    ] {
        let found =
            (given("review").records(vec![("/r", report(&wrong))])).with(json!({"head": HEAD}));
        assert_eq!(found.statuses(), review_statuses("missing"), "{wrong}");
    }
}

#[test]
fn the_review_gate_reads_the_head_from_the_right_side_of_a_three_dot_range() {
    let three = report(&json!({"range": format!("{BASE}...{HEAD}")}));
    let found = given("review")
        .records(vec![("/r", three)])
        .with(json!({"head": HEAD}));
    assert_eq!(found.statuses(), review_statuses("pass"));
}

#[test]
fn the_review_gate_refuses_a_diff_outside_the_planned_paths_and_passes_one_inside_them() {
    let reviewed =
        || (given("review").records(vec![("/r", report(&json!({})))])).with(json!({"head": HEAD}));
    assert_eq!(reviewed().check("paths"), ("pass", String::new()));
    let mut outside = reviewed();
    outside.outside = vec!["/repos/x/b.rs", "/repos/x/c.rs"];
    assert_eq!(
        outside.check("paths"),
        (
            "missing",
            "the change's diff edits files outside the plan's paths, so their rules were not in \
             the brief: /repos/x/b.rs, /repos/x/c.rs; add them to the paths with `board \
             changes plan`, or revert the edit"
                .to_owned()
        )
    );
    assert_eq!(outside.code("paths"), "changes::paths::outside_plan");
    let mut unplanned = reviewed();
    unplanned.planned = Vec::new();
    assert_eq!(
        unplanned.check("paths"),
        (
            "missing",
            "the change's plan declares no paths, so every edited file is outside them; write the \
             plan again with `board changes plan` and its paths"
                .to_owned()
        )
    );
    let mut unread = reviewed();
    unread.diff_unread = Some("git failed");
    assert_eq!(unread.check("paths"), ("missing", "git failed".to_owned()));
}

#[test]
fn the_land_gate_needs_the_test_sections_and_a_pull_request_record_and_done_has_no_gate() {
    assert_eq!(
        given("land").statuses(),
        statuses(&[
            ("test-sections", "missing"),
            ("pull-request", "missing"),
            ("lifecycle-paths-at-land", "pass"),
            ("lifecycle-none-paths-at-land", "pass"),
        ])
    );
    let tested = format!("{PLAN}{TESTS}");
    let mut unread = given("land").with(json!({"pulls": ["/runs/1"]}));
    unread.pull_read = false;
    assert_eq!(
        unread.check("pull-request").1,
        "the change's newest pull request /runs/1 does not read as a github/pull/v1 record; run \
         `board check /runs/1`, or record the pull request with `board github ref` and \
         attach it with --pull"
    );
    let landed = (given("land").plan(Some(&tested))).with(json!({"pulls": ["/gh/o/r/pulls/1"]}));
    assert_eq!(
        landed.statuses(),
        statuses(&[
            ("test-sections", "pass"),
            ("pull-request", "pass"),
            ("lifecycle-paths-at-land", "pass"),
            ("lifecycle-none-paths-at-land", "pass"),
        ])
    );
    assert!(gate(&given("done").observed()).is_empty());
}

/// The fixture plan with these UX and Data sections.
fn planned(ux: &str, data: &str) -> String {
    let head = PLAN.split("## UX").next().unwrap();
    format!(
        "{head}## UX\n\n{ux}\n\n## Data\n\n{data}\n\n## Failure paths\n\nNone: nothing can \
         fail.\n\n## Consumers\n\nNone: nothing reads it.\n\n## Lifecycle\n\nNone: nothing changes \
         state.\n"
    )
}

fn planned_ux(ux: &str) -> String {
    planned(ux, "None.")
}

fn plan_checks(plan: &str, records: Records) -> BTreeMap<String, &'static str> {
    (given("plan").plan(Some(plan)).records(records))
        .with(json!({"route": "owner"}))
        .statuses()
}

fn detail(plan: &str, name: &str, records: Records) -> String {
    (given("plan").plan(Some(plan)).records(records))
        .with(json!({"route": "owner"}))
        .check(name)
        .1
}

/// A decision whose `urls` name each URL and whose `refs` name each board
/// path in `named`.
fn agreed(named: &[&str]) -> Records {
    let urls: Vec<&str> = named
        .iter()
        .copied()
        .filter(|name| name.starts_with("http"))
        .collect();
    let refs: Vec<&str> = named
        .iter()
        .copied()
        .filter(|name| name.starts_with('/'))
        .collect();
    vec![(
        "/d/ux",
        (
            "plans/decision/v1",
            json!({"decision": "Agreed.", "urls": urls, "refs": refs}),
        ),
    )]
}

fn mockups_list() -> String {
    format!("### Mockups\n\n- The list view: {MOCKUP}\n- `/artifacts/m2`\n")
}

fn both() -> Records {
    agreed(&[MOCKUP, "/artifacts/m2"])
}

#[test]
fn the_plan_gate_needs_text_under_ux_and_data() {
    let bare = format!("{}## UX\n\n## Data\n", PLAN.split("## UX").next().unwrap());
    assert_eq!(
        detail(&bare, "plan-sections", Vec::new()),
        "the plan at /changes/t/plans/1 has no text under: UX, Data, Failure paths, Consumers, Lifecycle"
    );
    let plan = planned_ux("No UI change: it is headless.");
    assert_eq!(plan_checks(&plan, Vec::new())["plan-sections"], "pass");
}

#[test]
fn a_plan_with_no_ui_change_and_no_data_leaves_out_the_mockups_and_data_checks() {
    let checks = plan_checks(&planned_ux("No UI change: it is headless."), Vec::new());
    assert!(!checks.contains_key("mockups") && !checks.contains_key("data-signed"));
    let changed = plan_checks(&planned("A new list view.", "A new table."), Vec::new());
    assert!(changed.contains_key("mockups") && changed.contains_key("data-signed"));
}

#[test]
fn ux_that_says_no_ui_change_needs_a_reason() {
    assert_eq!(
        plan_checks(&planned_ux("No UI change."), Vec::new())["mockups"],
        "missing"
    );
    let reasoned = planned_ux("No UI change: it runs headless.");
    assert!(!plan_checks(&reasoned, Vec::new()).contains_key("mockups"));
}

#[test]
fn ux_with_no_mockups_list_is_missing_and_one_listing_agreed_mockups_passes() {
    let vague = planned_ux(&format!("A new list view.\n\n{NEIGHBOURS}"));
    assert_eq!(
        detail(&vague, "mockups", Vec::new()),
        "the UX section neither starts with No UI change and a reason nor lists mockups under a \
         ### Mockups heading"
    );
    let routed = (given("plan").plan(Some(&vague))).with(json!({"route": "owner"}));
    assert_eq!(routed.code("mockups"), "changes::ux::not_agreed");
    let listed = planned_ux(&format!(
        "A new list view.\n\n{}\n{NEIGHBOURS}",
        mockups_list()
    ));
    assert_eq!(plan_checks(&listed, both())["mockups"], "pass");
}

#[test]
fn only_the_mockups_list_names_mockups_so_a_route_or_an_issue_link_in_the_prose_is_not_one() {
    let prose =
        "The list opens at `/flows/:id`, as https://github.com/acme/widgets/issues/12 asks.";
    let plan = planned_ux(&format!("{prose}\n\n{}\n{NEIGHBOURS}", mockups_list()));
    assert_eq!(mockups(&sections(&plan)), vec![MOCKUP, "/artifacts/m2"]);
    assert_eq!(plan_checks(&plan, both())["mockups"], "pass");
}

#[test]
fn a_mockups_item_that_names_no_artifact_is_missing_and_one_that_names_one_passes() {
    let plan = planned_ux(&format!(
        "### Mockups\n\n- the list view, still to draw\n\n{NEIGHBOURS}"
    ));
    assert_eq!(
        detail(&plan, "mockups", Vec::new()),
        "this item of the Mockups list names no artifact by URL or board path: the list view, \
         still to draw"
    );
    let named = planned_ux(&format!("{}\n{NEIGHBOURS}", mockups_list()));
    assert_eq!(plan_checks(&named, both())["mockups"], "pass");
}

#[test]
fn a_mockup_with_no_decision_naming_it_is_not_agreed_and_one_named_in_urls_or_refs_is() {
    let plan = planned_ux(&format!("{}\n{NEIGHBOURS}", mockups_list()));
    assert_eq!(
        detail(&plan, "mockups", agreed(&[MOCKUP])),
        "these mockups have no attached plans/decision/v1 that names them in its urls or refs: \
         /artifacts/m2"
    );
    assert_eq!(plan_checks(&plan, both())["mockups"], "pass");
}

#[test]
fn a_decision_a_hook_wrote_agrees_nothing_and_one_an_agent_wrote_does() {
    let plan = planned_ux(&format!("{}\n{NEIGHBOURS}", mockups_list()));
    let mut stub = both();
    stub[0].1.1["written_by"] = json!("runs/pr-create");
    assert_eq!(plan_checks(&plan, stub)["mockups"], "missing");
    assert_eq!(plan_checks(&plan, both())["mockups"], "pass");
}

#[test]
fn ux_with_mockups_needs_a_neighbours_list_under_it_and_passes_with_one() {
    let bare = planned_ux(&mockups_list());
    assert_eq!(
        detail(&bare, "mockups", both()),
        "the UX section lists mockups but has no ### Neighbours heading under it with a list: \
         name the open changes and artifacts in the same area, and the components and flows the \
         design reuses"
    );
    let routed = (given("plan").plan(Some(&bare)).records(both())).with(json!({"route": "owner"}));
    assert_eq!(routed.code("mockups"), "changes::ux::no_neighbours");
    let headed = planned_ux(&format!(
        "{}\n### Neighbours\n\nNone found.\n",
        mockups_list()
    ));
    assert_eq!(plan_checks(&headed, both())["mockups"], "missing");
    let sibling = (planned_ux(&mockups_list()))
        .replace("## Data", "## Neighbours\n\n- /changes/other\n\n## Data");
    assert_eq!(plan_checks(&sibling, both())["mockups"], "missing");
    let listed = planned_ux(&format!("{}\n{NEIGHBOURS}", mockups_list()));
    assert_eq!(plan_checks(&listed, both())["mockups"], "pass");
}

#[test]
fn data_that_says_none_alone_leaves_out_the_data_check_and_none_of_followed_by_a_change_does_not() {
    let ux = "No UI change: it is headless.";
    for none in ["None", "None.", "None: it stores nothing."] {
        assert!(
            !plan_checks(&planned(ux, none), Vec::new()).contains_key("data-signed"),
            "{none}"
        );
    }
    let changed = planned(ux, "None of the tables change; we add a column.");
    assert_eq!(plan_checks(&changed, Vec::new())["data-signed"], "missing");
}

/// An understanding check of this change's data, with `fields` over it.
fn understanding(fields: &Value) -> (&'static str, Value) {
    let mut body = json!({"change": PATH, "subject": "data"});
    for (key, value) in fields.as_object().unwrap() {
        body[key] = value.clone();
    }
    ("changes/understanding/v1", body)
}

/// A data sign-off decision whose refs name `refs`.
fn signed(refs: &[&str]) -> (&'static str, Value) {
    (
        "plans/decision/v1",
        json!({"decision": "Signed off.", "refs": refs}),
    )
}

#[test]
fn data_that_is_not_none_needs_a_decision_referencing_this_changes_understanding_check() {
    let plan = planned(
        "No UI change: it is headless.",
        "One record per call, in the calls table.",
    );
    assert_eq!(
        detail(&plan, "data-signed", Vec::new()),
        "no attached plans/decision/v1 the owner signed references an attached \
         changes/understanding/v1 of this change's data; run the understanding-check skill, then \
         have the owner sign a decision whose refs name its record"
    );
    let routed = (given("plan").plan(Some(&plan))).with(json!({"route": "owner"}));
    assert_eq!(routed.code("data-signed"), "changes::data::not_signed");
    let unsigned = vec![(UNDERSTOOD, understanding(&json!({})))];
    assert_eq!(plan_checks(&plan, unsigned)["data-signed"], "missing");
    let other = vec![
        (
            UNDERSTOOD,
            understanding(&json!({"change": "/changes/other"})),
        ),
        ("/d/s", signed(&[UNDERSTOOD])),
    ];
    assert_eq!(plan_checks(&plan, other)["data-signed"], "missing");
    let ux = vec![
        (UNDERSTOOD, understanding(&json!({"subject": "ux"}))),
        ("/d/s", signed(&[UNDERSTOOD])),
    ];
    assert_eq!(plan_checks(&plan, ux)["data-signed"], "missing");
    let signed_off = vec![
        (UNDERSTOOD, understanding(&json!({}))),
        ("/d/s", signed(&[UNDERSTOOD])),
    ];
    assert_eq!(plan_checks(&plan, signed_off)["data-signed"], "pass");
}

#[test]
fn a_data_decision_a_hook_wrote_signs_nothing_and_one_the_owner_signed_does() {
    let plan = planned(
        "No UI change: it is headless.",
        "One record per call, in the calls table.",
    );
    let (kind, mut body) = signed(&[UNDERSTOOD]);
    body["written_by"] = json!("runs/pr-create");
    let stub = vec![
        (UNDERSTOOD, understanding(&json!({}))),
        ("/d/s", (kind, body)),
    ];
    assert_eq!(plan_checks(&plan, stub)["data-signed"], "missing");
    let owned = vec![
        (UNDERSTOOD, understanding(&json!({}))),
        ("/d/s", signed(&[UNDERSTOOD])),
    ];
    assert_eq!(plan_checks(&plan, owned)["data-signed"], "pass");
}

/// Rechecks, one per entry, each matching the plan or not.
fn rechecked(matches: &[bool]) -> Value {
    (matches.iter())
        .map(
            |matched| json!({"kind": "edge", "question": "Q?", "answer": "A.", "design": "A.", "matches": matched}),
        )
        .collect()
}

#[test]
fn the_build_gate_needs_two_matching_rechecks_on_each_attached_understanding_check() {
    let stored = planned("No UI change: it is headless.", "One record per call.");
    let built = |understood: (&'static str, Value)| {
        (given("build").plan(Some(&stored)).records(vec![
            (RUN, run(&json!({}))),
            (UNDERSTOOD, understood),
            ("/d/s", signed(&[UNDERSTOOD])),
        ]))
        .with(json!({"head": HEAD}))
    };
    for rechecks in [
        rechecked(&[]),
        rechecked(&[true]),
        rechecked(&[true, false]),
    ] {
        let found = built(understanding(&json!({"rechecks": rechecks.clone()})));
        assert_eq!(found.statuses()["rechecks"], "missing", "{rechecks}");
    }
    assert_eq!(
        built(understanding(&json!({}))).check("rechecks").1,
        "the understanding check at /changes/t/understanding/data holds fewer than two rechecks \
         that match the plan; before review, ask two of its predict or edge questions again, \
         worded differently, and add them to its rechecks"
    );
    let passed = built(understanding(
        &json!({"rechecks": rechecked(&[true, true])}),
    ));
    assert_eq!(
        passed.statuses(),
        statuses(&[
            ("head", "pass"),
            ("suite-run", "pass"),
            ("tasks", "pass"),
            ("rechecks", "pass")
        ])
    );
}

#[test]
fn an_attached_understanding_check_board_does_not_read_fails_the_recheck_and_another_record_does_not()
 {
    let stored = planned("No UI change: it is headless.", "One record per call.");
    let built = |kind: &'static str| {
        let other = "/changes/t/understanding/other";
        (given("build").plan(Some(&stored)).records(vec![
            (RUN, run(&json!({}))),
            (
                other,
                understanding(&json!({"rechecks": rechecked(&[true, true])})),
            ),
            ("/d/s", signed(&[other])),
        ]))
        .with(json!({"head": HEAD}))
        .unreadable(vec![(UNDERSTOOD, kind, "board::record::invalid")])
    };
    assert_eq!(
        built("changes/understanding/v1").check("rechecks"),
        (
            "missing",
            "the understanding check at /changes/t/understanding/data is degraded with \
             board::record::invalid, so it cannot be read; run `board check \
             /changes/t/understanding/data` and fix it"
                .to_owned()
        )
    );
    assert_eq!(built("tasks/task/v2").statuses()["rechecks"], "pass");
}

#[test]
fn a_heading_inside_a_code_fence_starts_no_section() {
    let found = sections("# T\n\n## Problem\n\n```\n## Outcome\n```\n");
    assert_eq!(found["problem"], "```\n## Outcome\n```");
    assert!(!found.contains_key("outcome"));
}

/// The plan fixture with its Lifecycle section replaced by `section`.
fn with_lifecycle(section: &str) -> String {
    let (before, _) = PLAN.rsplit_once("## Lifecycle").unwrap();
    format!("{before}## Lifecycle\n\n{section}\n")
}

const CONTROL: &str = "/controls/x/lifecycle-paths";

/// A change in plan whose plan says `section` under Lifecycle and whose
/// planned paths reach the lifecycle-paths control.
fn lifecycle_change(phase: &'static str, section: &str) -> Given {
    let mut change = given(phase).plan(Some(&with_lifecycle(section)));
    change.on_planned = vec![CONTROL];
    change.held = vec![CONTROL];
    change
}

#[test]
fn the_plan_gate_misses_each_new_section_when_left_out_or_empty_and_passes_none_and_a_reason() {
    let owned = |plan: &str| {
        given("plan")
            .with(json!({"route": "owner"}))
            .plan(Some(plan))
    };
    let cut = |heading: &str| {
        let (before, rest) = PLAN.split_once(heading).unwrap();
        let after = rest.split_once("## ").map_or("", |(_, after)| after);
        if after.is_empty() {
            before.to_owned()
        } else {
            format!("{before}## {after}")
        }
    };
    for (heading, name) in [
        ("## Failure paths", "Failure paths"),
        ("## Consumers", "Consumers"),
        ("## Lifecycle", "Lifecycle"),
    ] {
        assert_eq!(
            owned(&cut(heading)).check("plan-sections"),
            (
                "missing",
                format!("the plan at /changes/t/plans/1 has no text under: {name}")
            ),
            "{heading} left out"
        );
    }
    let empty = "## Failure paths\n\n## Consumers\n\nNone: nothing reads it.\n";
    let plan = PLAN.replace(
        "## Failure paths\n\nNone: nothing can fail.\n\n## Consumers\n\nNone: nothing reads it.\n",
        empty,
    );
    assert_eq!(
        owned(&plan).check("plan-sections"),
        (
            "missing",
            "the plan at /changes/t/plans/1 has no text under: Failure paths".to_owned()
        )
    );
    assert_eq!(owned(PLAN).statuses()["plan-sections"], "pass");
}

#[test]
fn lifecycle_declared_misses_none_when_the_planned_paths_reach_the_control_and_its_twins_pass() {
    assert_eq!(
        lifecycle_change("plan", "None: nothing changes state.").check("lifecycle-declared"),
        (
            "missing",
            "the plan's paths reach /controls/x/lifecycle-paths while its Lifecycle section \
             declares nothing"
                .to_owned()
        )
    );
    let unreached = {
        let mut change = lifecycle_change("plan", "None: nothing changes state.");
        change.on_planned = Vec::new();
        change
    };
    assert_eq!(
        unreached.check("lifecycle-declared"),
        ("pass", String::new())
    );
    let named = lifecycle_change("plan", "See `/changes/t/lifecycle`: three states.");
    assert!(!named.statuses().contains_key("lifecycle-declared"));
}

#[test]
fn lifecycle_declared_passes_and_says_no_control_exists_when_the_board_holds_none() {
    let mut change = lifecycle_change("plan", "None: nothing changes state.");
    change.on_planned = Vec::new();
    change.held = Vec::new();
    assert_eq!(
        change.check("lifecycle-declared"),
        (
            "pass",
            "no control /controls/x/lifecycle-paths exists, so the board names no such paths and \
             the check passes"
                .to_owned()
        )
    );
}

#[test]
fn lifecycle_record_misses_a_named_record_nobody_wrote_and_passes_one_that_is_there() {
    let mut named = lifecycle_change("plan", "See `/changes/t/lifecycle`: three states.");
    assert_eq!(
        named.check("lifecycle-record"),
        ("missing", "nothing is at /changes/t/lifecycle".to_owned())
    );
    named.existing = vec![("/changes/t/lifecycle", true)];
    assert_eq!(named.check("lifecycle-record"), ("pass", String::new()));
    let none = lifecycle_change("plan", "None: nothing changes state.");
    assert!(!none.statuses().contains_key("lifecycle-record"));
}

/// "None" counts only as Data's pattern counts it: a word of its own followed
/// by a full stop, a colon or nothing.
#[test]
fn lifecycle_none_reads_as_data_none_does_so_a_sentence_that_starts_with_none_is_no_opt_out() {
    let sentence = lifecycle_change("plan", "None of the states change, and one is added.");
    let checks = sentence.statuses();
    assert!(!checks.contains_key("lifecycle-declared"), "{checks:?}");
    assert_eq!(checks["lifecycle-record"], "missing", "{checks:?}");
    let bare = lifecycle_change("plan", "None");
    assert!(bare.statuses().contains_key("lifecycle-declared"));
}

const LIFECYCLE_EDIT: &str =
    "the change's diff edits files that /controls/x/lifecycle-paths list: /repos/x/src/sync.rs";

/// The plan text with the Lifecycle heading left out.
fn without_lifecycle() -> String {
    PLAN.split_once("\n## Lifecycle").unwrap().0.to_owned()
}

#[test]
fn lifecycle_paths_in_review_misses_an_edit_with_no_record_whatever_the_section_says() {
    let cases = [
        with_lifecycle("None: nothing changes state."),
        with_lifecycle("See `/changes/t/lifecycle`: three states."),
        without_lifecycle(),
    ];
    for plan in &cases {
        let mut change = given("review").plan(Some(plan));
        change.controlled = vec![(CONTROL, vec!["/repos/x/src/sync.rs"])];
        assert_eq!(
            change.check("lifecycle-paths"),
            ("missing", LIFECYCLE_EDIT.to_owned()),
            "{plan}"
        );
        change.controlled = vec![(CONTROL, Vec::new())];
        assert_eq!(
            change.check("lifecycle-paths"),
            ("pass", String::new()),
            "{plan}"
        );
    }
}

#[test]
fn lifecycle_paths_in_review_is_left_out_once_a_lifecycle_record_exists_whatever_the_section_says()
{
    let cases = [
        with_lifecycle("None: nothing changes state."),
        with_lifecycle("See `/changes/t/lifecycle`: three states."),
        without_lifecycle(),
    ];
    for plan in &cases {
        let mut change = given("review").plan(Some(plan));
        change.controlled = vec![(CONTROL, vec!["/repos/x/src/sync.rs"])];
        change.records_held = vec!["/changes/t/lifecycle"];
        assert!(!change.statuses().contains_key("lifecycle-paths"), "{plan}");
        if *plan == cases[0] {
            assert_eq!(
                change.check("lifecycle-none-paths"),
                ("missing", LIFECYCLE_EDIT.to_owned()),
                "{plan}"
            );
        } else {
            assert!(
                !change.statuses().contains_key("lifecycle-none-paths"),
                "{plan}"
            );
        }
    }
}

/// A `when` needs every one of its conditions.
#[test]
fn a_check_with_absent_and_a_section_applies_only_when_both_hold() {
    let observed = |plan: &str, held: Vec<&'static str>| {
        let mut change = given("review").plan(Some(plan));
        change.records_held = held;
        change.controlled = vec![(CONTROL, Vec::new())];
        let mut observed = change.observed();
        let flow = observed.flow.as_mut().unwrap();
        let review = (flow.phases.iter_mut())
            .find(|phase| phase.name == "review")
            .unwrap();
        let check = (review.checks.iter_mut())
            .find(|each| each.id == "lifecycle-paths")
            .unwrap();
        check.when.as_mut().unwrap().section = Some(BTreeMap::from([(
            "Lifecycle".to_owned(),
            crate::lifecycle::NONE.to_owned(),
        )]));
        observed
    };
    let applies = |observed: &Observed| {
        gate(observed)
            .iter()
            .any(|found| found.check == "lifecycle-paths")
    };
    let none = with_lifecycle("None: nothing changes state.");
    let named = with_lifecycle("See `/changes/t/lifecycle`: three states.");
    assert!(applies(&observed(&none, Vec::new())));
    assert!(!applies(&observed(&named, Vec::new())));
    assert!(!applies(&observed(&none, vec!["/changes/t/lifecycle"])));
}

#[test]
fn the_lifecycle_paths_remedy_says_to_write_the_record_or_revert_the_edit() {
    let mut change = lifecycle_change("review", "None: nothing changes state.");
    change.controlled = vec![(CONTROL, vec!["/repos/x/src/sync.rs"])];
    let remedy = change.remedy("lifecycle-paths");
    assert!(remedy.contains("`board changes lifecycle`"), "{remedy}");
    assert!(remedy.contains("revert the edit"), "{remedy}");
}

/// `board changes lifecycle --path` ignores stdin, and `external` goes only
/// on stdin, so a remedy that names the flag names a command Board refuses.
#[test]
fn every_remedy_that_names_changes_lifecycle_names_the_stdin_form_and_no_flag() {
    let flow = development();
    let remedies: Vec<&str> = (flow.phases.iter().flat_map(|phase| phase.checks.iter()))
        .filter_map(|each| each.remedy.as_deref())
        .filter(|remedy| remedy.contains("changes lifecycle"))
        .collect();
    assert_eq!(remedies.len(), 6, "{remedies:?}");
    for remedy in remedies {
        assert!(!remedy.contains("changes lifecycle --path"), "{remedy}");
        assert!(
            remedy.contains(
                "piping JSON with `path`, `text`, `doc` and `external` to `board changes lifecycle`"
            ),
            "{remedy}"
        );
    }
}

#[test]
fn the_lifecycle_record_remedy_says_none_colon_followed_by_the_reason() {
    let named = lifecycle_change("plan", "See `/changes/t/lifecycle`: three states.");
    let remedy = named.remedy("lifecycle-record");
    assert!(
        remedy.contains("start the section with `None:` followed by the reason"),
        "{remedy}"
    );
}

#[test]
fn the_flow_and_the_brief_read_a_lifecycle_of_none_with_one_pattern() {
    let flow = development();
    let plan = flow.phase("plan").unwrap();
    let when = |id: &str| {
        let check = plan.checks.iter().find(|each| each.id == id).unwrap();
        check.when.as_ref().unwrap()
    };
    assert_eq!(
        when("lifecycle-declared").section.as_ref().unwrap()["Lifecycle"],
        crate::lifecycle::NONE
    );
    assert_eq!(
        when("lifecycle-record").section_not.as_ref().unwrap()["Lifecycle"],
        crate::lifecycle::NONE
    );
}

#[test]
fn the_development_review_names_a_reviewer_with_tools_and_a_checkout_and_not_the_toolless_run() {
    let change = given("review");
    let flow = development();
    let review = flow.phase("review").unwrap();
    assert!(
        review
            .instruction
            .contains("a reviewer in a fresh agent with tools and a checkout of {head}"),
        "{}",
        review.instruction
    );
    assert!(
        !review.instruction.contains("board review run"),
        "{}",
        review.instruction
    );
    let remedy = change.remedy("review");
    assert!(remedy.contains("tools and a checkout"), "{remedy}");
    assert!(!remedy.contains("board review run"), "{remedy}");
}

#[test]
fn a_when_absent_reads_the_references_and_the_suite_only_when_its_path_names_it() {
    let read = |path: &str| {
        let check: crate::flow::Check = serde_json::from_value(json!({
            "id": "x",
            "kind": "field",
            "field": "head",
            "when": {"absent": {"path": path, "type": "a/b/v1"}},
        }))
        .unwrap();
        reads(&[check])
    };
    let plain = read("/changes/{name}/x");
    assert!(plain.contains(&Read::Absent));
    assert!(plain.contains(&Read::References));
    assert!(!plain.contains(&Read::Suite));
    assert!(read("{suite}/x").contains(&Read::Suite));
}

/// A stored flow version may still name `changes/run/v1` in a records check,
/// so an attempt stored as either type answers a check that names either.
#[test]
fn a_records_check_reads_an_attempt_whichever_type_it_has_and_not_a_record_of_another_type() {
    let referenced = |kind: &str| crate::change::Referenced {
        records: vec![(
            "/changes/t/runs/1".to_owned(),
            (kind.to_owned(), Map::new()),
        )],
        ..Default::default()
    };
    for stored in ["changes/attempt/v1", "changes/run/v1"] {
        for asked in ["changes/attempt/v1", "changes/run/v1"] {
            assert_eq!(
                referenced(stored).of_type(asked).count(),
                1,
                "{stored} {asked}"
            );
        }
        assert_eq!(
            referenced(stored).of_type("changes/stop/v1").count(),
            0,
            "{stored}"
        );
    }
    assert_eq!(
        referenced("changes/stop/v1")
            .of_type("changes/run/v1")
            .count(),
        0
    );
}
