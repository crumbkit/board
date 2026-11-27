//! Each phase's gate in the shipped development flow, as a pure function
//! of the change and the records it references. These were the Python
//! plugin's `test_gates.py`; the check ids follow the flow.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use super::design::mockups;
use super::{Observed, Read, Status, applies, fails_unread, gate, reads, sections};
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
    /// The rules `rules.for-paths` finds on the plan's paths.
    on_planned: Vec<&'static str>,
    /// The rules the board holds.
    held: Vec<&'static str>,
    /// What is at each path an `exists` check names.
    existing: Vec<(&'static str, bool)>,
    /// The paths a `when.absent` names that hold the record it means.
    records_held: Vec<&'static str>,
    /// The files a rule-paths rule lists among the edited ones.
    controlled: Vec<(&'static str, Vec<&'static str>)>,
    /// The plan's typed answers: none to each question, as the fixture
    /// plan's text says.
    answers: Value,
    /// Whether the change follows the stored version of the development
    /// flow whose checks read the plan's prose.
    prose: bool,
    /// The number of points the flow's `rechecks` check wants, when it is
    /// not the shipped flow's.
    rechecks: Option<u32>,
}

/// The version of the development flow shipped before plans gave typed
/// answers, which a change started under it still follows.
const PROSE_FLOW: &str = include_str!("../flow/flows-md/development-60bd988ebac76d79.json");

/// The typed answers of a plan that stores data, so its plan needs the
/// owner's approval.
const STORES: &str = r#"{"ux": "none", "data": "stored", "lifecycle": "none"}"#;

/// A change in `phase` whose plan stores data, so owner-approval applies.
fn approving(phase: &'static str) -> Given {
    given(phase).answers(serde_json::from_str(STORES).unwrap())
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
        answers: json!({"ux": "none", "data": "none", "lifecycle": "none"}),
        prose: false,
        rechecks: None,
    }
}

impl Given {
    fn records(mut self, records: Records) -> Self {
        self.records = records;
        self
    }

    /// The plan gives these typed answers.
    fn answers(mut self, answers: Value) -> Self {
        self.answers = answers;
        self
    }

    /// The change follows the stored flow version that reads prose, and its
    /// plan, written before plans gave typed answers, gives none.
    fn prose(mut self) -> Self {
        self.prose = true;
        self.answers(json!({}))
    }

    /// The flow's `rechecks` check wants `wanted` points.
    fn rechecks(mut self, wanted: u32) -> Self {
        self.rechecks = Some(wanted);
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
        observed.flow = Some(if self.prose {
            serde_json::from_str(PROSE_FLOW).unwrap()
        } else {
            development()
        });
        if let (Some(wanted), Some(flow)) = (self.rechecks, observed.flow.as_mut()) {
            for check in (flow.phases.iter_mut()).flat_map(|phase| phase.checks.iter_mut()) {
                if check.rechecks.is_some() {
                    check.rechecks = Some(wanted);
                }
            }
        }
        observed.answers = serde_json::from_value(self.answers.clone()).unwrap();
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
            .map(|(rule, files)| {
                let files = files.iter().map(|each| (*each).to_owned()).collect();
                ((*rule).to_owned(), Some(files))
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

/// The person a fixture decision of the owner's names in `by`.
pub(crate) const PERSON: &str = "pat";

/// The four ways a decision says who wrote it, each with whether an owner
/// check counts it: a `by` of a program and session only, an empty
/// person, a hook's stub that names a person, and the owner's.
fn signers() -> [(&'static str, Value, bool); 4] {
    [
        (
            "no person",
            json!({"by": {"program": "claude-code", "session": "s"}}),
            false,
        ),
        (
            "empty person",
            json!({"by": {"person": "", "program": "claude-code"}}),
            false,
        ),
        (
            "hook stub",
            json!({"by": {"person": PERSON}, "written_by": "hook"}),
            false,
        ),
        ("the owner's", json!({"by": {"person": PERSON}}), true),
    ]
}

/// Whether `counts` passes each of [`signers`], by name, beside whether
/// it should, so one assertion shows every case that differs.
pub(crate) fn judged(counts: impl Fn(&Value) -> bool) -> [Vec<(&'static str, bool)>; 2] {
    compared(signers(), counts)
}

/// The ways an answer says who wrote it, each with whether an owner check
/// counts it: one that names a person, or a surface of the owner's in
/// `written_by`, counts; one with neither, or with either not a string,
/// does not.
fn answerers() -> [(&'static str, Value, bool); 8] {
    [
        (
            "no person",
            json!({"by": {"program": "claude-code", "session": "s"}}),
            false,
        ),
        (
            "empty person",
            json!({"by": {"person": "", "program": "claude-code"}}),
            false,
        ),
        (
            "the pane's",
            json!({"by": {"session": "local_1"}, "written_by": "plans/pane-answer"}),
            true,
        ),
        (
            "/ask's with a person",
            json!({"by": {"person": PERSON}, "written_by": "claude-code/ask"}),
            true,
        ),
        ("the owner's", json!({"by": {"person": PERSON}}), true),
        ("a person not a string", json!({"by": {"person": 1}}), false),
        (
            "written_by not a string",
            json!({"by": {"session": "s"}, "written_by": ["plans/pane-answer"]}),
            false,
        ),
        (
            "empty written_by",
            json!({"by": {"session": "s"}, "written_by": ""}),
            false,
        ),
    ]
}

/// Whether `counts` passes each of [`answerers`], by name, beside whether
/// it should.
pub(crate) fn answered(counts: impl Fn(&Value) -> bool) -> [Vec<(&'static str, bool)>; 2] {
    compared(answerers(), counts)
}

fn compared<const N: usize>(
    table: [(&'static str, Value, bool); N],
    counts: impl Fn(&Value) -> bool,
) -> [Vec<(&'static str, bool)>; 2] {
    let (got, wanted) = (table.into_iter())
        .map(|(signer, fields, counted)| ((signer, counts(&fields)), (signer, counted)))
        .unzip();
    [got, wanted]
}

/// What the owner-approval check of the fixture change says when nothing
/// passes it.
const UNMET: &str = "no plans/decision/v1 that the owner signed with by.person whose refs name \
     the plan or the change is attached, nor a plans/answer/v1 from the owner (one that names \
     `by.person`, or was written through the owner's questions pane, Board.app or /ask) whose \
     chosen option, in its attached question, names /changes/t, nor the owner's Touch ID \
     approval of changes.approve for the current plan";

/// `body` with each of `fields` over it.
pub(crate) fn by(mut body: Value, fields: &Value) -> Value {
    for (key, value) in fields.as_object().unwrap() {
        body[key] = value.clone();
    }
    body
}

/// A decision the owner signed whose refs name the change's plan.
fn approval() -> (&'static str, (&'static str, Value)) {
    (
        "/d/plan",
        (
            "plans/decision/v1",
            json!({"refs": ["/changes/t/plans/1"], "by": {"person": PERSON}}),
        ),
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
fn on_every_route_the_plan_gate_needs_a_persons_decision_whose_refs_name_the_plan_or_the_change() {
    for route in ["owner", "ask-first", "unowned"] {
        let asking = || approving("plan").with(json!({"route": route}));
        let unrelated = vec![(
            "/d",
            (
                "plans/decision/v1",
                json!({"refs": ["/changes/other"], "by": {"person": PERSON}}),
            ),
        )];
        assert_eq!(
            asking().records(unrelated).statuses()["owner-approval"],
            "missing"
        );
        for named in ["/changes/t/plans/1", "/changes/t/plans/1.md", PATH] {
            let [got, wanted] = judged(|fields| {
                let body = by(json!({"refs": [named]}), fields);
                let decided = vec![("/d", ("plans/decision/v1", body))];
                asking().records(decided).statuses()["owner-approval"] == "pass"
            });
            assert_eq!(got, wanted, "{route} {named}");
        }
    }
}

/// An attached /ask question whose option `a` approves `named` and whose
/// option `b` names no change, and an attached answer that chose `choice`,
/// with `fields` over it. The answer's words always name the change.
fn answer(named: &str, choice: &str, fields: &Value) -> Records {
    let options = json!([
        {"id": "a", "text": format!("Approve the plan at {named}.")},
        {"id": "b", "text": "Hold the work for now."},
    ]);
    let question = json!({"question": "Approve?", "options": options});
    let words = format!("Approve the plan at {PATH}.");
    let body = json!({"question": "/q/1", "choice": choice, "words": words});
    vec![
        ("/q/1", ("plans/question/v1", question)),
        ("/a/1", ("plans/answer/v1", by(body, fields))),
    ]
}

#[test]
fn the_owners_answer_whose_chosen_option_names_the_change_meets_owner_approval() {
    let approval = |records| {
        (approving("plan").with(json!({"route": "owner"})))
            .records(records)
            .check("owner-approval")
    };
    let [got, wanted] = answered(|fields| approval(answer(PATH, "a", fields)).0 == "pass");
    assert_eq!(got, wanted);
    let owners = json!({"by": {"person": PERSON}});
    let unmet = ("missing", UNMET.to_owned());
    let paths = [
        "/changes/other",
        "/changes/t-two",
        "/changes/tea",
        "/team/changes/t",
    ];
    for named in paths {
        assert_eq!(approval(answer(named, "a", &owners)), unmet, "{named}");
    }
    // The other option, though the answer's words name the change.
    assert_eq!(approval(answer(PATH, "b", &owners)), unmet);
    let program = json!({"by": {"program": "claude-code"}});
    assert_eq!(approval(answer(PATH, "a", &program)), unmet);
    // The answer alone, without its question attached.
    let alone = answer(PATH, "a", &owners).split_off(1);
    assert_eq!(approval(alone), unmet);
    assert_eq!(approval(answer(PATH, "a", &owners)).0, "pass");
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
fn the_review_gate_passes_a_diff_outside_the_planned_paths_and_names_the_files_it_adds() {
    let reviewed =
        || (given("review").records(vec![("/r", report(&json!({})))])).with(json!({"head": HEAD}));
    assert_eq!(reviewed().check("paths"), ("pass", String::new()));
    let mut outside = reviewed();
    outside.outside = vec!["/repos/x/b.rs", "/repos/x/c.rs"];
    assert_eq!(
        outside.check("paths"),
        (
            "pass",
            "these files are outside the plan's paths, and the change adds them to its paths \
             without a new plan: /repos/x/b.rs, /repos/x/c.rs. Their rules were not in the \
             review brief. `board changes pr` lists them in the pull request body."
                .to_owned()
        )
    );
    let mut many = reviewed();
    many.outside = vec![
        "/repos/x/0.rs",
        "/repos/x/1.rs",
        "/repos/x/2.rs",
        "/repos/x/3.rs",
        "/repos/x/4.rs",
        "/repos/x/5.rs",
        "/repos/x/6.rs",
        "/repos/x/7.rs",
        "/repos/x/8.rs",
        "/repos/x/9.rs",
        "/repos/x/10.rs",
        "/repos/x/11.rs",
    ];
    let detail = many.check("paths").1;
    assert!(detail.contains("/repos/x/9.rs, and 2 more."), "{detail}");
    assert!(!detail.contains("/repos/x/10.rs"), "{detail}");
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
    assert_eq!(unread.code("paths"), "changes::paths::outside_plan");
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
    (given("plan").prose().plan(Some(plan)).records(records))
        .with(json!({"route": "owner"}))
        .statuses()
}

fn detail(plan: &str, name: &str, records: Records) -> String {
    (given("plan").prose().plan(Some(plan)).records(records))
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
            json!({"decision": "Agreed.", "urls": urls, "refs": refs, "by": {"person": PERSON}}),
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
    let routed = (given("plan").prose().plan(Some(&vague))).with(json!({"route": "owner"}));
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
        "these mockups have neither an attached plans/decision/v1 the owner signed that names \
         them in its urls or refs, nor an attached plans/answer/v1 from the owner whose chosen \
         option, in its attached question, names them: /artifacts/m2"
    );
    assert_eq!(plan_checks(&plan, both())["mockups"], "pass");
}

#[test]
fn a_mockup_decision_agrees_only_when_it_names_a_person_and_no_hook_wrote_it() {
    let plan = planned_ux(&format!("{}\n{NEIGHBOURS}", mockups_list()));
    let [got, wanted] = judged(|fields| {
        let mut decided = both();
        decided[0].1.1 = by(decided[0].1.1.clone(), fields);
        plan_checks(&plan, decided)["mockups"] == "pass"
    });
    assert_eq!(got, wanted);
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
    let routed =
        (given("plan").prose().plan(Some(&bare)).records(both())).with(json!({"route": "owner"}));
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
        json!({"decision": "Signed off.", "refs": refs, "by": {"person": PERSON}}),
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
        "no attached plans/decision/v1 the owner signed with by.person references an attached \
         changes/understanding/v1 of this change's data, nor does an attached answer from the \
         owner choose an option naming the change and that record; run the understanding-check \
         skill, then have the owner sign a decision with by.person whose refs name its record, or \
         answer the question `board changes ask` files"
    );
    let routed = (given("plan").prose().plan(Some(&plan))).with(json!({"route": "owner"}));
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
fn a_data_decision_signs_off_only_when_it_names_a_person_and_no_hook_wrote_it() {
    let plan = planned(
        "No UI change: it is headless.",
        "One record per call, in the calls table.",
    );
    let [got, wanted] = judged(|fields| {
        let (kind, body) = signed(&[UNDERSTOOD]);
        let decided = vec![
            (UNDERSTOOD, understanding(&json!({}))),
            ("/d/s", (kind, by(body, fields))),
        ];
        plan_checks(&plan, decided)["data-signed"] == "pass"
    });
    assert_eq!(got, wanted);
}

#[test]
fn the_build_gate_reads_rechecks_only_under_a_sign_off_that_names_a_person() {
    let stored = planned("No UI change: it is headless.", "One record per call.");
    let [got, wanted] = judged(|fields| {
        let (kind, body) = signed(&[UNDERSTOOD]);
        let built = (given("build").prose().plan(Some(&stored)).records(vec![
            (RUN, run(&json!({}))),
            (
                UNDERSTOOD,
                understanding(&json!({"rechecks": rechecked(&[true, true])})),
            ),
            ("/d/s", (kind, by(body, fields))),
        ]))
        .with(json!({"head": HEAD}));
        built.statuses()["rechecks"] == "pass"
    });
    assert_eq!(got, wanted);
}

/// Rechecks, one per entry, each matching the plan or not, each of its own
/// point.
fn rechecked(matches: &[bool]) -> Value {
    (matches.iter().enumerate())
        .map(|(at, matched)| {
            json!({
                "kind": "edge", "question": "Q?", "answer": "A.", "design": "A.",
                "matches": matched, "point": format!("point-{at}"),
            })
        })
        .collect()
}

#[test]
fn the_build_gate_needs_two_matching_rechecks_on_each_attached_understanding_check() {
    let stored = planned("No UI change: it is headless.", "One record per call.");
    let built = |understood: (&'static str, Value)| {
        (given("build").prose().plan(Some(&stored)).records(vec![
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
        "the understanding check at /changes/t/understanding/data holds rechecks of fewer than two \
         points; ask two of its predict or edge questions again, worded differently, and add \
         each to its rechecks with its own point"
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
        (given("build").prose().plan(Some(&stored)).records(vec![
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

const LIFECYCLE_RULE: &str = "/rules/x/lifecycle-paths";

/// A change in plan whose plan says `section` under Lifecycle and whose
/// planned paths reach the lifecycle-paths rule.
fn lifecycle_change(phase: &'static str, section: &str) -> Given {
    let mut change = given(phase).prose().plan(Some(&with_lifecycle(section)));
    change.on_planned = vec![LIFECYCLE_RULE];
    change.held = vec![LIFECYCLE_RULE];
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
fn lifecycle_declared_misses_none_when_the_planned_paths_reach_the_rule_and_its_twins_pass() {
    assert_eq!(
        lifecycle_change("plan", "None: nothing changes state.").check("lifecycle-declared"),
        (
            "missing",
            "the plan's paths reach /rules/x/lifecycle-paths while its Lifecycle section \
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
fn lifecycle_declared_passes_and_says_no_rule_exists_when_the_board_holds_none() {
    let mut change = lifecycle_change("plan", "None: nothing changes state.");
    change.on_planned = Vec::new();
    change.held = Vec::new();
    assert_eq!(
        change.check("lifecycle-declared"),
        (
            "pass",
            "no rule /rules/x/lifecycle-paths exists, so the board names no such paths and \
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
    "the change's diff edits files that /rules/x/lifecycle-paths list: /repos/x/src/sync.rs";

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
        let mut change = given("review").prose().plan(Some(plan));
        change.controlled = vec![(LIFECYCLE_RULE, vec!["/repos/x/src/sync.rs"])];
        assert_eq!(
            change.check("lifecycle-paths"),
            ("missing", LIFECYCLE_EDIT.to_owned()),
            "{plan}"
        );
        change.controlled = vec![(LIFECYCLE_RULE, Vec::new())];
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
        let mut change = given("review").prose().plan(Some(plan));
        change.controlled = vec![(LIFECYCLE_RULE, vec!["/repos/x/src/sync.rs"])];
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
        change.controlled = vec![(LIFECYCLE_RULE, Vec::new())];
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

/// `changes.owed` skips a check on `fails_unread` alone, so it must fail
/// exactly the checks whose route or answers make `applies` false, and
/// leave a `when` with a section pattern to the gate.
#[test]
fn fails_unread_fails_a_when_exactly_where_the_gate_finds_it_does_not_apply() {
    let whens = [
        json!({}),
        json!({"route": ["owner"]}),
        json!({"route": ["owner", "ask-first"]}),
        json!({"answers": {"data": "stored"}}),
        json!({"answers": {"ux": "mockups", "data": "stored"}}),
        json!({"answers_not": {"data": "none"}}),
        json!({"answers_not": {"data": "none", "lifecycle": "none"}}),
        json!({"route": ["unowned"], "answers": {"data": "stored"}}),
        json!({"route": ["owner"], "answers_not": {"ux": "none"}}),
    ];
    let routes = [None, Some("owner"), Some("ask-first"), Some("unowned")];
    let answers = [
        json!({}),
        json!({"data": "none"}),
        json!({"data": "stored"}),
        json!({"ux": "mockups", "data": "stored", "lifecycle": "none"}),
        json!({"ux": "none", "data": "none", "lifecycle": "none"}),
    ];
    let mut compared = 0;
    for when in &whens {
        let check: crate::flow::Check =
            serde_json::from_value(json!({"id": "c", "kind": "tasks", "when": when})).unwrap();
        for route in routes {
            for given in &answers {
                let mut observed = given_route(route);
                observed.answers = serde_json::from_value(given.clone()).unwrap();
                let skipped = fails_unread(&check, observed.change.route, Some(observed.answers));
                let gated = applies(&observed, &BTreeMap::new(), &check) == Ok(false);
                assert_eq!(
                    skipped, gated,
                    "when {when}, route {route:?}, answers {given}"
                );
                compared += 1;
            }
        }
    }
    assert_eq!(compared, 9 * 4 * 5);
}

/// The twin: a `when` with a section pattern fails unread only off its
/// route, whatever the answers, and with no answers only the route is read.
#[test]
fn fails_unread_leaves_a_when_with_a_section_pattern_or_no_answers_to_the_gate_on_its_route() {
    let check = |when: Value| -> crate::flow::Check {
        serde_json::from_value(json!({"id": "c", "kind": "tasks", "when": when})).unwrap()
    };
    let stored = serde_json::from_value(json!({"data": "stored"})).unwrap();
    let sectioned = check(json!({
        "route": ["owner"],
        "section": {"Data": "stored"},
        "answers": {"data": "none"},
    }));
    let owner = given_route(Some("owner")).change.route;
    let unowned = given_route(Some("unowned")).change.route;
    assert!(!fails_unread(&sectioned, owner, Some(stored)));
    assert!(fails_unread(&sectioned, unowned, Some(stored)));
    let answered = check(json!({"route": ["owner"], "answers": {"data": "none"}}));
    assert!(!fails_unread(&answered, owner, None));
    assert!(fails_unread(&answered, owner, Some(stored)));
    assert!(fails_unread(&answered, unowned, None));
}

/// A change in review with `route`, or none.
fn given_route(route: Option<&str>) -> Observed {
    let change = given("review");
    match route {
        Some(route) => change.with(json!({"route": route})).observed(),
        None => change.observed(),
    }
}

#[test]
fn the_lifecycle_paths_remedy_says_to_write_the_record_or_revert_the_edit() {
    let mut change = lifecycle_change("review", "None: nothing changes state.");
    change.controlled = vec![(LIFECYCLE_RULE, vec!["/repos/x/src/sync.rs"])];
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
    let flow: crate::flow::Flow = serde_json::from_str(PROSE_FLOW).unwrap();
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

/// An `obligations` check reads the diff, so the obligations on a file the
/// change adds to its plan's paths count in a phase with no `paths` check.
#[test]
fn an_obligations_check_reads_the_diff_with_the_plan_and_the_obligations() {
    let check: crate::flow::Check =
        serde_json::from_value(json!({"id": "o", "kind": "obligations"})).unwrap();
    let read = reads(&[check]);
    for wanted in [Read::Plan, Read::Diff, Read::Obligations] {
        assert!(read.contains(&wanted), "{wanted:?} in {read:?}");
    }
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

mod answers;
mod design;
mod owner_answers;
mod rechecks;
mod review_head;

/// At land, a plan that lacks the test sections takes them through
/// `changes plan-amend`, which adds only those headings, so the land gate's
/// remedy and instruction name it and not the whole-plan resend; the plan
/// phase's missing sections still take `changes plan`.
#[test]
fn the_land_test_sections_remedy_names_plan_amend_and_the_plan_sections_remedy_names_changes_plan()
{
    let land = given("land").remedy("test-sections");
    assert!(
        land.contains(
            "`board changes plan-amend --path /changes/t --proves <text> --does-not-prove <text>`"
        ),
        "{land}"
    );
    assert!(!land.contains("changes plan --path"), "{land}");
    let plan = given("plan").plan(Some("# T\n")).remedy("plan-sections");
    assert!(
        plan.contains("`board changes plan --path /changes/t`"),
        "{plan}"
    );
    assert!(!plan.contains("plan-amend"), "{plan}");
    let flow = development();
    let instruction = &flow.phase("land").unwrap().instruction;
    assert!(
        instruction.contains("`board changes plan-amend --path {change}`"),
        "{instruction}"
    );
}
