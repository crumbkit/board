"""Each phase's gate, as a pure function of the change and the records it references."""

from typing import TYPE_CHECKING, Any

from conftest import BASE, HEAD, PLAN, TASK

from board_changes.gates import Observed, gate

if TYPE_CHECKING:
    from board_changes.change import Change

PATH = "/changes/t"
SUITE = "/suites/checks"
RUN = f"{SUITE}/runs/1"


def observed(
    phase: str,
    records: dict[str, tuple[str, dict[str, Any]]] | None = None,
    plan: str | None = PLAN,
    suite: str | None = SUITE,
    **fields: Any,  # noqa: ANN401 - the change's other fields
) -> Observed:
    """What a gate reads for a change in `phase` with a plan at /changes/t/plan.md."""
    change: Change = {"$type": "changes/change/v1", "title": "T", "mount": "/repos/x", "phase": phase, "task": TASK}
    change["plan"] = "/changes/t/plan.md"
    change.update(fields)  # type: ignore[typeddict-item]
    return Observed(path=PATH, change=change, records=records or {}, plan=plan, suite=suite)


def statuses(found: Observed) -> dict[str, str]:
    """Each check's status, by name."""
    return {check["check"]: check["status"] for check in gate(found)}


def run(**fields: Any) -> tuple[str, dict[str, Any]]:  # noqa: ANN401 - the run's other fields
    """A full run of a clean tree of the check suite on the head commit, which passed."""
    body: dict[str, Any] = {
        "scope": "full",
        "dirty": False,
        "versions": {"repo": HEAD[:7]},
        "results": [],
    }
    return "runs/run/v1", body | fields


def report(**fields: Any) -> tuple[str, dict[str, Any]]:  # noqa: ANN401 - the report's other fields
    """A review report of `base..head` that comments, with one medium finding."""
    body: dict[str, Any] = {
        "range": f"{BASE}..{HEAD}",
        "patch_ids": ["p1"],
        "findings": [{"severity": "medium", "text": "x"}],
        "verdict": "comment",
    }
    return "review/report/v1", body | fields


def test_the_plan_gate_passes_when_each_plan_section_has_text_and_misses_when_one_is_empty() -> None:
    assert statuses(observed("plan", route="owner")) == {"plan": "pass", "task": "pass", "route": "pass"}
    [plan, *_] = gate(
        observed("plan", plan="# T\n\n## Problem\n\nX.\n\n## Outcome\n\n## out of scope\nY.\n", route="owner")
    )
    assert (plan["status"], plan["detail"]) == ("missing", "the plan at /changes/t/plan.md has no text under: Outcome")


def test_the_plan_gate_misses_a_plan_file_that_is_not_there_and_passes_one_with_text() -> None:
    [plan, *_] = gate(observed("plan", plan=None, route="owner"))
    assert (plan["status"], plan["detail"]) == ("missing", "no plan file is at /changes/t/plan.md")
    assert statuses(observed("plan", plan=PLAN, route="owner"))["plan"] == "pass"


def test_the_plan_gate_misses_a_change_with_no_route_and_passes_each_route() -> None:
    [*_, route] = gate(observed("plan"))
    assert (route["status"], route["detail"]) == (
        "missing",
        "the change records no route; set one with `board call changes.attach --path /changes/t --route <route>`",
    )
    for named in ("owner", "ask-first", "unowned"):
        assert statuses(observed("plan", route=named))["route"] == "pass", named


def test_on_the_ask_first_route_the_plan_gate_needs_a_decision_whose_refs_name_the_plan_or_the_change() -> None:
    unrelated = {"/d": ("plans/decision/v1", {"refs": ["/changes/other"]})}
    assert statuses(observed("plan", unrelated, route="ask-first"))["owner-approval"] == "missing"
    for named in ("/changes/t/plan.md", PATH):
        approved = {"/d": ("plans/decision/v1", {"refs": [named]})}
        assert statuses(observed("plan", approved, route="ask-first"))["owner-approval"] == "pass", named


def test_the_build_gate_passes_with_tasks_done_or_dropped_and_a_passing_run_on_the_head() -> None:
    records = {
        "/t/a.md": ("tasks/task/v2", {"status": "done"}),
        "/t/b.md": ("tasks/task/v2", {"status": "dropped"}),
        RUN: run(),
    }
    assert statuses(observed("build", records, head=HEAD)) == {"tasks": "pass", "check-run": "pass"}


def test_the_build_gate_misses_each_task_still_open() -> None:
    records = {"/t/a.md": ("tasks/task/v2", {"status": "open"}), "/t/b.md": ("tasks/task/v2", {"status": "deferred"})}
    [tasks, _] = gate(observed("build", records | {RUN: run()}, head=HEAD))
    assert tasks["detail"] == "these tasks are not done or dropped: /t/a.md, /t/b.md"


def test_the_build_gate_misses_a_run_that_failed_or_is_dirty_targeted_of_another_suite_or_another_commit() -> None:
    for wrong in (
        {"results": [{"case": "a", "outcome": "fail", "note": "x"}]},
        {"results": [{"case": "a", "outcome": "error", "note": "x"}]},
        {"dirty": True},
        {"dirty": None},
        {"scope": "targeted"},
        {"versions": {"repo": "b4d"}},
        {"versions": {"repo": "0000000"}},
    ):
        assert statuses(observed("build", {RUN: run(**wrong)}, head=HEAD))["check-run"] == "missing", wrong
    skipped = {"results": [{"case": "a", "outcome": "skipped", "note": "x"}]}
    assert statuses(observed("build", {RUN: run(**skipped)}, head=HEAD))["check-run"] == "pass"


def test_a_run_belongs_to_the_suite_whose_runs_directory_holds_it_whatever_its_suite_field_says() -> None:
    for elsewhere in ("/suites/other/runs/1", "/suites/checks-2/runs/1", "/suites/checks/1", "/runs/1"):
        found = observed("build", {elsewhere: run(suite=SUITE)}, head=HEAD)
        assert statuses(found)["check-run"] == "missing", elsewhere
    assert statuses(observed("build", {RUN: run(suite="/suites/other")}, head=HEAD))["check-run"] == "pass"


def test_the_build_gate_says_when_no_suite_is_named_or_no_head_is_recorded() -> None:
    [_, no_suite] = gate(observed("build", {RUN: run()}, suite=None, head=HEAD))
    assert no_suite["detail"] == "no contributing doc in /repos/x names a runs/suite/v1 path as its check suite"
    [_, no_head] = gate(observed("build", {RUN: run()}))
    assert no_head["detail"] == "the change records no head commit; attach one with --head"


def test_the_review_gate_passes_a_report_on_the_head_without_a_blocking_finding() -> None:
    assert statuses(observed("review", {"/r": report()}, head=HEAD[:12])) == {"review": "pass"}
    for wrong in (
        {"range": f"{BASE}..{'b' * 40}"},
        {"range": HEAD},
        {"verdict": "changes"},
        {"findings": [{"severity": "high", "text": "x"}]},
        {"findings": [{"severity": "critical", "text": "x"}]},
    ):
        assert statuses(observed("review", {"/r": report(**wrong)}, head=HEAD)) == {"review": "missing"}, wrong


def test_the_review_gate_reads_the_head_from_the_right_side_of_a_three_dot_range() -> None:
    assert statuses(observed("review", {"/r": report(range=f"{BASE}...{HEAD}")}, head=HEAD)) == {"review": "pass"}


def test_the_land_gate_needs_the_pull_request_url_and_done_has_no_gate() -> None:
    assert statuses(observed("land")) == {"pull-request": "missing"}
    assert statuses(observed("land", pull_request="https://github.com/o/r/pull/1")) == {"pull-request": "pass"}
    assert gate(observed("done")) == []
