"""Each phase's gate, computed only from the change and the records it references."""

import re
from dataclasses import dataclass, field
from typing import Any, Literal, TypedDict

from board_changes.change import Change

TASK_TYPE = "tasks/task/v2"
RUN_TYPE = "runs/run/v1"
# A decision and the field that names what it rests on. plans/decision/v1 is read until the runs mod stops
# writing stubs of it.
DECISIONS = {"decisions/decision/v1": "basis", "plans/decision/v1": "refs"}
REPORT_TYPE = "review/report/v1"
# The plan's sections the plan gate requires, by heading, compared without case.
SECTIONS = ("Problem", "Outcome", "Out of scope")
# Findings at these severities block the review gate.
BLOCKING = ("critical", "high")
# The fewest hex digits a commit is named by when compared with another.
SHORT_COMMIT = 7
HEADING = re.compile(r"^#{1,6}\s+(.*?)\s*#*\s*$")


class Check(TypedDict):
    """One requirement of a gate: `pass`, or `missing` with what is missing."""

    check: str
    status: Literal["pass", "missing"]
    detail: str


@dataclass(frozen=True)
class Observed:
    """What a gate reads: the change, its records by logical path as (type, body), the plan's text and the suite.

    A referenced record Board marks degraded is left out of `records`, so it
    satisfies nothing; `unreadable` maps each reference left out to the code
    that explains it, `board::path::not_found` when nothing is there. A file
    that is not a record has type `file` and a directory type `directory`,
    each with an empty body. `plan` is `None` when no file is at the plan's
    path.
    `suite` is the repository's check suite, or `None` when its contributing
    docs name none; only the build gate reads it.
    """

    path: str
    change: Change
    records: dict[str, tuple[str, dict[str, Any]]]
    plan: str | None
    suite: str | None
    unreadable: dict[str, str] = field(default_factory=dict[str, str])


def check(name: str, missing: str | None) -> Check:
    """A check that passes when nothing is `missing`."""
    if missing is None:
        return Check(check=name, status="pass", detail="")
    return Check(check=name, status="missing", detail=missing)


def gate(observed: Observed) -> list[Check]:
    """The checks for leaving the change's phase. `done` and `dropped` have none."""
    phase = observed.change["phase"]
    if phase == "plan":
        checks = [check("plan", plan_missing(observed)), check("route", route_missing(observed))]
        if observed.change.get("route") == "ask-first":
            checks.append(check("owner-approval", approval_missing(observed)))
        return checks
    if phase == "build":
        return [check("tasks", tasks_missing(observed)), check("check-run", run_missing(observed))]
    if phase == "review":
        return [check("review", review_missing(observed))]
    if phase == "land":
        missing = None if observed.change.get("pull_request") else "the change records no pull request URL"
        return [check("pull-request", missing)]
    return []


def of_type(observed: Observed, type_name: str) -> list[tuple[str, dict[str, Any]]]:
    """The referenced records of `type_name`, by path, in the order the change references them."""
    return [(path, body) for path, (found, body) in observed.records.items() if found == type_name]


def plan_missing(observed: Observed) -> str | None:
    """What the plan lacks: the file, or a section with no text under its heading."""
    plan = observed.change.get("plan")
    if plan is None or observed.plan is None:
        return f"no plan file is at {plan}" if plan else "the change names no plan file"
    filled: set[str] = set()
    heading: str | None = None
    for line in observed.plan.splitlines():
        found = HEADING.match(line)
        if found:
            heading = found.group(1).casefold()
        elif heading is not None and line.strip():
            filled.add(heading)
    empty = [section for section in SECTIONS if section.casefold() not in filled]
    return f"the plan at {plan} has no text under: {', '.join(empty)}" if empty else None


def route_missing(observed: Observed) -> str | None:
    """Whether the change records a route."""
    if observed.change.get("route"):
        return None
    return (
        f"the change records no route; set one with `board call changes.attach --path {observed.path} --route <route>`"
    )


def approval_missing(observed: Observed) -> str | None:
    """Whether a decision approving the plan is attached: one whose `basis` (or old `refs`) names the plan or change."""
    approves = {observed.path, observed.change.get("plan")}
    for type_name, field_name in DECISIONS.items():
        for _, body in of_type(observed, type_name):
            refs: list[Any] = body.get(field_name) or []
            if approves.intersection(ref for ref in refs if isinstance(ref, str)):
                return None
    return "no decisions/decision/v1 whose basis names the plan or the change is attached"


def tasks_missing(observed: Observed) -> str | None:
    """The attached tasks that are neither done nor dropped."""
    left = [path for path, body in of_type(observed, TASK_TYPE) if body.get("status") not in ("done", "dropped")]
    return f"these tasks are not done or dropped: {', '.join(left)}" if left else None


def same_commit(one: object, other: str) -> bool:
    """Whether `one` names the commit `other` names, either possibly abbreviated."""
    if not isinstance(one, str) or min(len(one), len(other)) < SHORT_COMMIT:
        return False
    return one.startswith(other) or other.startswith(one)


def run_missing(observed: Observed) -> str | None:
    """Whether a passing full run of a clean tree, of the check suite on the head commit, is attached.

    A run belongs to the suite whose `runs/` directory holds it, as the runs
    plugin counts runs.
    """
    head = observed.change.get("head")
    if observed.suite is None:
        return f"no contributing doc in {observed.change['mount']} names a runs/suite/v1 path as its check suite"
    if head is None:
        return "the change records no head commit; attach one with --head"
    runs = observed.suite + "/runs/"
    for path, body in of_type(observed, RUN_TYPE):
        versions: dict[str, Any] = body.get("versions") or {}
        results: list[dict[str, Any]] = body.get("results") or []
        if (
            path.startswith(runs)
            and body.get("scope") == "full"
            and body.get("dirty") is False
            and any(same_commit(version, head) for version in versions.values())
            and not any(result.get("outcome") in ("fail", "error") for result in results)
        ):
            return None
    return f"no attached run of {observed.suite} on {head} is full, clean and passing"


def reviewed_head(report: dict[str, Any]) -> str | None:
    """The head commit a report covers: the right side of its `base..head` range."""
    covered = report.get("range")
    if not isinstance(covered, str) or ".." not in covered:
        return None
    return covered.rsplit("..", 1)[1].lstrip(".")


def review_missing(observed: Observed) -> str | None:
    """Whether an attached review report covers the head, does not ask for changes and has no blocking finding."""
    head = observed.change.get("head")
    if head is None:
        return "the change records no head commit; attach one with --head"
    for _, body in of_type(observed, REPORT_TYPE):
        findings: list[dict[str, Any]] = body.get("findings") or []
        if (
            same_commit(reviewed_head(body), head)
            and body.get("verdict") != "changes"
            and not any(f.get("severity") in BLOCKING for f in findings)
        ):
            return None
    return (
        f"no attached review/report/v1 covers {head} with a verdict of approve or comment"
        " and no critical or high finding"
    )
