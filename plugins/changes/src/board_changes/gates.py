"""Each phase's gate, computed only from the change and the records it references."""

import re
from dataclasses import dataclass, field
from typing import Any, Literal, TypedDict

from board_changes.change import TASK_TYPE, UNDERSTANDING_TYPE, Change, kind_of

RUN_TYPE = "runs/run/v1"
DECISION_TYPE = "plans/decision/v1"
REPORT_TYPE = "review/report/v1"
# The plan's sections the plan gate requires, by heading, compared without case.
SECTIONS = ("Problem", "Outcome", "Out of scope", "UX", "Data")
# How a UX section says it lists no mockups, and a Data section that it changes no data, at its start, without case.
NO_UI = re.compile(r"^no ui change\b", re.IGNORECASE)
NO_DATA = re.compile(r"^none\b", re.IGNORECASE)
# A mockup a UX section lists: a URL, or a board path in inline code.
MOCKUP_URL = re.compile(r"https?://[^\s)>\]]+")
MOCKUP_PATH = re.compile(r"`(/[^`\s]+)`")
LIST_ITEM = re.compile(r"^\s*(?:[-*+]|\d+[.)])\s+\S", re.MULTILINE)
# Findings at these severities block the review gate.
BLOCKING = ("critical", "high")
# The fewest hex digits a commit is named by when compared with another.
SHORT_COMMIT = 7
HEADING = re.compile(r"^#{1,6}\s+(.*?)\s*#*\s*$")
# Lines that open or close a Markdown code fence.
FENCES = ("```", "~~~")
# The refusal `advance` gives when a check with its own code is the first that is missing; any other is
# `changes::gate::unmet`.
CODES = {
    "verdict": "changes::prototype::never_lands",
    "ux": "changes::ux::not_agreed",
    "neighbours": "changes::ux::no_neighbours",
    "data": "changes::data::not_signed",
}


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


def sections(text: str) -> dict[str, str]:
    """The text under each heading of a Markdown document, by heading without case, trimmed.

    A section runs to the next heading of the same or a higher level, so it
    holds its subsections. A line inside a code fence is never a heading.
    """
    found: dict[str, list[str]] = {}
    opened: list[tuple[int, str]] = []
    fenced = False
    for line in text.splitlines():
        match = None if fenced else HEADING.match(line)
        if line.lstrip().startswith(FENCES):
            fenced = not fenced
        level = len(line) - len(line.lstrip("#"))
        while match and opened and opened[-1][0] >= level:
            opened.pop()
        for _, heading in opened:
            found[heading].append(line)
        if match:
            heading = str(match.group(1)).casefold()
            found.setdefault(heading, [])
            opened.append((level, heading))
    return {name: "\n".join(lines).strip() for name, lines in found.items()}


def check(name: str, missing: str | None) -> Check:
    """A check that passes when nothing is `missing`."""
    if missing is None:
        return Check(check=name, status="pass", detail="")
    return Check(check=name, status="missing", detail=missing)


def gate(observed: Observed) -> list[Check]:
    """The checks for leaving the change's phase. `done` and `dropped` have none."""
    phase = observed.change["phase"]
    if kind_of(observed.change) == "prototype" and phase == "build":
        return [check("verdict", verdict_missing(observed))]
    if phase == "plan":
        checks = [
            check("plan", plan_missing(observed)),
            check("task", task_missing(observed)),
            check("route", route_missing(observed)),
        ]
        if observed.change.get("route") == "ask-first":
            checks.append(check("owner-approval", approval_missing(observed)))
        found = sections(observed.plan or "")
        return [
            *checks,
            check("ux", ux_missing(observed, found)),
            check("neighbours", neighbours_missing(found)),
            check("data", data_missing(observed, found)),
        ]
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
    found = sections(observed.plan)
    empty = [section for section in SECTIONS if not found.get(section.casefold())]
    return f"the plan at {plan} has no text under: {', '.join(empty)}" if empty else None


def task_missing(observed: Observed) -> str | None:
    """Whether the change names the task it does."""
    if observed.change.get("task"):
        return None
    return (
        "the change names no task; add one with `board call tasks.add` and set it with "
        f"`board call changes.attach --path {observed.path} --task <path>`"
    )


def verdict_missing(observed: Observed) -> str:
    """Why a prototype never passes its build gate: its verdict ends it instead."""
    return (
        "a prototype never advances or lands; when it has answered its question, end it with "
        f"`board call changes.verdict --path {observed.path} --state settled --verdict '<one sentence>'`"
    )


def mockups(ux: str) -> list[str]:
    """The mockups a UX section lists before its Neighbours, each once, in order: URLs and board paths in code."""
    listed = re.split(r"^#{1,6}\s+neighbours\b", ux, maxsplit=1, flags=re.IGNORECASE | re.MULTILINE)[0]
    urls = [url.rstrip(".,;:") for url in MOCKUP_URL.findall(listed)]
    return list(dict.fromkeys([*urls, *MOCKUP_PATH.findall(listed)]))


def named_by_decisions(observed: Observed) -> set[str]:
    """Every URL and board path an attached decision names in its `urls` or `refs`."""
    named: set[str] = set()
    for _, body in of_type(observed, DECISION_TYPE):
        for key in ("urls", "refs"):
            values: list[Any] = body.get(key) or []
            named.update(value for value in values if isinstance(value, str))
    return named


def ux_missing(observed: Observed, found: dict[str, str]) -> str | None:
    """Whether the UX section says No UI change, or lists mockups that an attached decision names each of."""
    ux = found.get("ux", "")
    if NO_UI.match(ux):
        return None
    listed = mockups(ux)
    if not listed:
        return (
            "the UX section neither starts with No UI change nor lists a mockup artifact by URL or by a board path "
            "in inline code"
        )
    unagreed = [mockup for mockup in listed if mockup not in named_by_decisions(observed)]
    if unagreed:
        return "these mockups have no attached plans/decision/v1 that names them in its urls or refs: " + ", ".join(
            unagreed
        )
    return None


def neighbours_missing(found: dict[str, str]) -> str | None:
    """Whether a UX section that lists mockups holds a Neighbours heading with a list under it."""
    ux = found.get("ux", "")
    neighbours = found.get("neighbours", "")
    if NO_UI.match(ux) or not mockups(ux) or (neighbours in ux and LIST_ITEM.search(neighbours)):
        return None
    return (
        "the UX section lists mockups but has no Neighbours heading with a list: name the open changes and "
        "artifacts in the same area, and the components and flows the design reuses"
    )


def data_missing(observed: Observed, found: dict[str, str]) -> str | None:
    """Whether the Data section says None, or an attached decision references this change's data understanding."""
    if NO_DATA.match(found.get("data", "")):
        return None
    understood = {
        path
        for path, body in of_type(observed, UNDERSTANDING_TYPE)
        if body.get("change") == observed.path and body.get("subject") == "data"
    }
    for _, body in of_type(observed, DECISION_TYPE):
        refs: list[Any] = body.get("refs") or []
        if understood.intersection(ref for ref in refs if isinstance(ref, str)):
            return None
    return (
        "the Data section is not None, and no attached plans/decision/v1 references an attached "
        f"{UNDERSTANDING_TYPE} of this change's data; run the understanding-check skill, then have the owner "
        "sign a decision whose refs name its record"
    )


def route_missing(observed: Observed) -> str | None:
    """Whether the change records a route."""
    if observed.change.get("route"):
        return None
    return (
        f"the change records no route; set one with `board call changes.attach --path {observed.path} --route <route>`"
    )


def approval_missing(observed: Observed) -> str | None:
    """Whether a decision approving the plan is attached: one whose `refs` name the plan or the change."""
    approves = {observed.path, observed.change.get("plan")}
    for _, body in of_type(observed, DECISION_TYPE):
        refs: list[Any] = body.get("refs") or []
        if approves.intersection(ref for ref in refs if isinstance(ref, str)):
            return None
    return "no plans/decision/v1 whose refs name the plan or the change is attached"


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
