"""Each phase's gate, as a pure function of the change and the records it references."""

from typing import TYPE_CHECKING, Any

from conftest import BASE, HEAD, PLAN, TASK

from board_changes.gates import Observed, gate, mockups, sections

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
    assert set(statuses(observed("plan", route="owner")).values()) == {"pass"}
    [plan, *_] = gate(
        observed("plan", plan="# T\n\n## Problem\n\nX.\n\n## Outcome\n\n## out of scope\nY.\n", route="owner")
    )
    assert (plan["status"], plan["detail"]) == (
        "missing",
        "the plan at /changes/t/plan.md has no text under: Outcome, UX, Data",
    )


def test_the_plan_gate_misses_a_plan_file_that_is_not_there_and_passes_one_with_text() -> None:
    [plan, *_] = gate(observed("plan", plan=None, route="owner"))
    assert (plan["status"], plan["detail"]) == ("missing", "no plan file is at /changes/t/plan.md")
    assert statuses(observed("plan", plan=PLAN, route="owner"))["plan"] == "pass"


def test_the_plan_gate_misses_a_change_with_no_route_and_passes_each_route() -> None:
    [route] = [found for found in gate(observed("plan")) if found["check"] == "route"]
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
    assert statuses(observed("build", records, head=HEAD)) == {"tasks": "pass", "check-run": "pass", "recheck": "pass"}


def test_the_build_gate_misses_each_task_still_open() -> None:
    records = {"/t/a.md": ("tasks/task/v2", {"status": "open"}), "/t/b.md": ("tasks/task/v2", {"status": "deferred"})}
    [tasks, *_] = gate(observed("build", records | {RUN: run()}, head=HEAD))
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
    [_, no_suite, _] = gate(observed("build", {RUN: run()}, suite=None, head=HEAD))
    assert no_suite["detail"] == "no contributing doc in /repos/x names a runs/suite/v1 path as its check suite"
    [_, no_head, _] = gate(observed("build", {RUN: run()}))
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


MOCKUP = "https://claude.ai/artifact/m1"
NEIGHBOURS = "### Neighbours\n\n- /changes/flows-list, the same area.\n"
UNDERSTOOD = "/changes/t/understanding/data"


def planned(ux: str = "No UI change: it is headless.", data: str = "None.") -> str:
    """The fixture plan with these UX and Data sections."""
    return PLAN.split("## UX")[0] + f"## UX\n\n{ux}\n\n## Data\n\n{data}\n"


def plan_checks(plan: str, records: dict[str, tuple[str, dict[str, Any]]] | None = None) -> dict[str, str]:
    """Each plan-gate check's status for an owner-routed change with `plan`."""
    return statuses(observed("plan", records, plan=plan, route="owner"))


def agreed(*named: str) -> dict[str, tuple[str, dict[str, Any]]]:
    """A decision whose `urls` name each URL and whose `refs` name each board path in `named`."""
    urls = [name for name in named if name.startswith("http")]
    refs = [name for name in named if name.startswith("/")]
    return {"/d/ux": ("plans/decision/v1", {"decision": "Agreed.", "urls": urls, "refs": refs})}


def detail(plan: str, name: str, records: dict[str, tuple[str, dict[str, Any]]] | None = None) -> str:
    """The detail of the plan gate's check `name`."""
    [found] = [c for c in gate(observed("plan", records, plan=plan, route="owner")) if c["check"] == name]
    return found["detail"]


def test_the_plan_gate_needs_text_under_ux_and_data() -> None:
    bare = PLAN.split("## UX")[0] + "## UX\n\n## Data\n"
    assert detail(bare, "plan") == "the plan at /changes/t/plan.md has no text under: UX, Data"
    assert plan_checks(planned())["plan"] == "pass"


def test_a_plan_with_no_ui_change_and_no_data_passes_the_ux_neighbours_and_data_checks() -> None:
    checks = plan_checks(planned())
    assert (checks["ux"], checks["neighbours"], checks["data"]) == ("pass", "pass", "pass")


MOCKUPS = f"### Mockups\n\n- The list view: {MOCKUP}\n- `/artifacts/m2`\n"
BOTH = agreed(MOCKUP, "/artifacts/m2")


def test_ux_that_says_no_ui_change_needs_a_reason() -> None:
    assert plan_checks(planned(ux="No UI change."))["ux"] == "missing"
    assert plan_checks(planned(ux="No UI change: it runs headless."))["ux"] == "pass"


def test_ux_with_no_mockups_list_is_missing_and_one_listing_agreed_mockups_passes() -> None:
    vague = planned(ux=f"A new list view.\n\n{NEIGHBOURS}")
    assert detail(vague, "ux") == (
        "the UX section neither starts with No UI change and a reason nor lists mockups under a ### Mockups heading"
    )
    assert plan_checks(planned(ux=f"A new list view.\n\n{MOCKUPS}\n{NEIGHBOURS}"), BOTH)["ux"] == "pass"


def test_only_the_mockups_list_names_mockups_so_a_route_or_an_issue_link_in_the_prose_is_not_one() -> None:
    prose = "The list opens at `/flows/:id`, as https://github.com/acme/widgets/issues/12 asks."
    plan = planned(ux=f"{prose}\n\n{MOCKUPS}\n{NEIGHBOURS}")
    assert mockups(sections(plan)) == [MOCKUP, "/artifacts/m2"]
    assert plan_checks(plan, BOTH)["ux"] == "pass"


def test_a_mockups_item_that_names_no_artifact_is_missing_and_one_that_names_one_passes() -> None:
    plan = planned(ux=f"### Mockups\n\n- the list view, still to draw\n\n{NEIGHBOURS}")
    assert (
        detail(plan, "ux")
        == "this item of the Mockups list names no artifact by URL or board path: the list view, still to draw"
    )
    assert plan_checks(planned(ux=f"{MOCKUPS}\n{NEIGHBOURS}"), BOTH)["ux"] == "pass"


def test_a_mockup_with_no_decision_naming_it_is_not_agreed_and_one_named_in_urls_or_refs_is() -> None:
    plan = planned(ux=f"{MOCKUPS}\n{NEIGHBOURS}")
    assert detail(plan, "ux", agreed(MOCKUP)) == (
        "these mockups have no attached plans/decision/v1 that names them in its urls or refs: /artifacts/m2"
    )
    assert plan_checks(plan, BOTH)["ux"] == "pass"


def test_a_decision_a_hook_wrote_agrees_nothing_and_one_an_agent_wrote_does() -> None:
    plan = planned(ux=f"{MOCKUPS}\n{NEIGHBOURS}")
    [(path, (kind, body))] = BOTH.items()
    stub = {path: (kind, body | {"written_by": "runs/pr-create"})}
    assert plan_checks(plan, stub)["ux"] == "missing"
    assert plan_checks(plan, BOTH)["ux"] == "pass"


def test_ux_with_mockups_needs_a_neighbours_list_under_it_and_passes_with_one() -> None:
    bare = planned(ux=MOCKUPS)
    assert detail(bare, "neighbours", BOTH) == (
        "the UX section lists mockups but has no ### Neighbours heading under it with a list: name the open "
        "changes and artifacts "
        "in the same area, and the components and flows the design reuses"
    )
    headed = planned(ux=f"{MOCKUPS}\n### Neighbours\n\nNone found.\n")
    assert plan_checks(headed, BOTH)["neighbours"] == "missing"
    sibling = planned(ux=MOCKUPS).replace("## Data", "## Neighbours\n\n- /changes/other\n\n## Data")
    assert plan_checks(sibling, BOTH)["neighbours"] == "missing"
    assert plan_checks(planned(ux=f"{MOCKUPS}\n{NEIGHBOURS}"), BOTH)["neighbours"] == "pass"


def test_data_that_says_none_alone_passes_and_none_of_followed_by_a_change_does_not() -> None:
    for none in ("None", "None.", "None: it stores nothing."):
        assert plan_checks(planned(data=none))["data"] == "pass", none
    assert plan_checks(planned(data="None of the tables change; we add a column."))["data"] == "missing"


def understanding(**fields: Any) -> tuple[str, dict[str, Any]]:  # noqa: ANN401 - the record's other fields
    """An understanding check of this change's data."""
    return "changes/understanding/v1", {"change": PATH, "subject": "data"} | fields


def signed(*refs: str) -> tuple[str, dict[str, Any]]:
    """A data sign-off decision whose refs name `refs`."""
    return "plans/decision/v1", {"decision": "Signed off.", "refs": list(refs)}


def test_data_that_is_not_none_needs_a_decision_referencing_this_changes_understanding_check() -> None:
    plan = planned(data="One record per call, in the calls table.")
    assert detail(plan, "data") == (
        "the Data section is not None, and no attached plans/decision/v1 references an attached "
        "changes/understanding/v1 of this change's data; run the understanding-check skill, then have the owner "
        "sign a decision whose refs name its record"
    )
    unsigned = {UNDERSTOOD: understanding()}
    assert plan_checks(plan, unsigned)["data"] == "missing"
    other = {UNDERSTOOD: understanding(change="/changes/other"), "/d/s": signed(UNDERSTOOD)}
    assert plan_checks(plan, other)["data"] == "missing"
    ux = {UNDERSTOOD: understanding(subject="ux"), "/d/s": signed(UNDERSTOOD)}
    assert plan_checks(plan, ux)["data"] == "missing"
    assert plan_checks(plan, {UNDERSTOOD: understanding(), "/d/s": signed(UNDERSTOOD)})["data"] == "pass"


def test_a_data_decision_a_hook_wrote_signs_nothing_and_one_the_owner_signed_does() -> None:
    plan = planned(data="One record per call, in the calls table.")
    kind, body = signed(UNDERSTOOD)
    stub = {UNDERSTOOD: understanding(), "/d/s": (kind, body | {"written_by": "runs/pr-create"})}
    assert plan_checks(plan, stub)["data"] == "missing"
    assert plan_checks(plan, {UNDERSTOOD: understanding(), "/d/s": signed(UNDERSTOOD)})["data"] == "pass"


def rechecked(matches: list[bool]) -> list[dict[str, Any]]:
    """Rechecks, one per entry, each matching the plan or not."""
    return [{"kind": "edge", "question": "Q?", "answer": "A.", "matches": match} for match in matches]


def test_the_build_gate_needs_two_matching_rechecks_on_each_attached_understanding_check() -> None:
    done = {RUN: run()}
    for rechecks in ([], rechecked([True]), rechecked([True, False])):
        found = observed("build", done | {UNDERSTOOD: understanding(rechecks=rechecks)}, head=HEAD)
        assert statuses(found)["recheck"] == "missing", rechecks
    [check] = [
        c for c in gate(observed("build", done | {UNDERSTOOD: understanding()}, head=HEAD)) if c["check"] == "recheck"
    ]
    assert check["detail"] == (
        "the understanding check at /changes/t/understanding/data holds fewer than two rechecks that match the "
        "plan; before review, ask two of its predict or edge questions again, worded differently, and add them to "
        "its rechecks"
    )
    passed = observed("build", done | {UNDERSTOOD: understanding(rechecks=rechecked([True, True]))}, head=HEAD)
    assert statuses(passed) == {"tasks": "pass", "check-run": "pass", "recheck": "pass"}
