"""A change's steps, derived from which records exist, and the `next` capability that shows them.

Nothing about a change's progress is stored: each step is done when a
record of its kind satisfies the kind's rule, so no field goes stale when a
session stops partway.
"""

import json
from dataclasses import dataclass
from typing import Any, cast

from board_plugin.board import Board, literal
from board_plugin.protocol import CallRequest, PluginError

from board_changes.change import (
    CHECK_TYPE,
    FILES_TYPE,
    LANDED_TYPE,
    PLAN_TYPE,
    PLUGIN,
    READABLE,
    RUN_TYPE,
    Change,
    Records,
    read,
    records,
    referenced,
    same_commit,
)
from board_changes.workflow import APPROVE, PERSONAL, steps_of

QUESTION_TYPE = "plans/question/v1"
ANSWER_TYPE = "plans/answer/v1"
BRIEF_TYPE = "review/brief/v1"
REPORT_TYPE = "review/report/v1"
# Findings at these severities keep a review from completing its step.
BLOCKING = ("critical", "high")
# The most controls one `controls.check` lists, so every control that adds a step is seen.
CONTROLS_LIMIT = 10_000


@dataclass(frozen=True)
class Seen:
    """What the steps are derived from: the change, its records, its head and who references it."""

    path: str
    change: Change
    held: Records
    head: str | None
    # Records that reference the change, by the field that does: (logical path, type, body).
    citing: list[tuple[str, str, dict[str, Any]]]


def head_of(board: Board, change: Change, held: Records) -> str | None:
    """The commit the newest check's run names under the repo record's name, or None without one."""
    newest = held.newest(CHECK_TYPE)
    if newest is None:
        return None
    run = newest[1]["run"]
    found, _ = referenced(board, [run])
    versions: dict[str, Any] = found.get(run, ("", {}))[1].get("versions") or {}
    commit = versions.get(repo_name(change))
    return commit if isinstance(commit, str) else None


def repo_name(change: Change) -> str:
    """The repo record's name, the last segment of its path, under which a run names the commit it tested."""
    return change["repo"].rstrip("/").rsplit("/", 1)[-1]


def citing(
    board: Board, paths: list[str], pointer: str, types: tuple[str, ...]
) -> list[tuple[str, str, dict[str, Any]]]:
    """The readable records of `types` that reference one of `paths` from `pointer` or an item under it."""
    sources: list[str] = []
    for answer in board.refs(paths):
        for each in (answer or {}).get("to", []):
            pointers = [str(at) for at in each.get("pointers", [])]
            if any(at == pointer or at.startswith(pointer + "/") for at in pointers):
                sources.append(str(each["logical"]))
    if not sources:
        return []
    listed = ", ".join(literal(source) for source in dict.fromkeys(sources))
    kinds = ", ".join(literal(kind) for kind in types)
    rows = board.rows(
        f"SELECT logical, type, body FROM records WHERE logical IN ({listed}) AND type IN ({kinds})"  # noqa: S608 - values are quoted by literal()
        f" AND {READABLE} ORDER BY logical"
    )
    return [(str(logical), str(kind), cast("dict[str, Any]", json.loads(body))) for logical, kind, body in rows]


def plan_done(board: Board, seen: Seen) -> str | None:
    """The newest plan, when every reference in its `uses` resolves."""
    newest = seen.held.newest(PLAN_TYPE)
    if newest is None:
        return None
    uses = [str(ref) for ref in newest[1]["uses"]]
    found, _ = referenced(board, uses)
    return newest[0] if all(ref in found for ref in uses) else None


def answer_done(board: Board, seen: Seen) -> str | None:
    """An answer whose `choice` is `approve` to a question whose `for` names the change."""
    questions = [path for path, kind, _ in seen.citing if kind == QUESTION_TYPE]
    if not questions:
        return None
    answers = citing(board, questions, "/question", (ANSWER_TYPE,))
    return next((path for path, _, body in answers if body.get("choice") == "approve"), None)


def check_done(board: Board, seen: Seen) -> str | None:
    """The newest check, when its run is a full, clean run of the change's suite with no fail or error."""
    newest = seen.held.newest(CHECK_TYPE)
    if newest is None:
        return None
    run = str(newest[1]["run"])
    found, _ = referenced(board, [run])
    kind, body = found.get(run, ("", {}))
    results: list[dict[str, Any]] = body.get("results") or []
    passed = (
        kind == RUN_TYPE
        and run.startswith(seen.change["suite"] + "/runs/")
        and body.get("scope") == "full"
        and body.get("dirty") is False
        and not any(result.get("outcome") in ("fail", "error") for result in results)
    )
    return newest[0] if passed else None


def reviewed_head(report: dict[str, Any]) -> str | None:
    """The head commit a report covers: the right side of its `base..head` range."""
    covered = report.get("range")
    if not isinstance(covered, str) or ".." not in covered:
        return None
    return covered.rsplit("..", 1)[1].lstrip(".")


def review_done(board: Board, seen: Seen, step: str) -> str | None:
    """An approving report with no high finding, answering a brief for this change and step, that ends at the head."""
    briefs = [path for path, kind, body in seen.citing if kind == BRIEF_TYPE and body.get("step") == step]
    if seen.head is None or not briefs:
        return None
    for path, _, report in citing(board, briefs, "/brief", (REPORT_TYPE,)):
        findings: list[dict[str, Any]] = report.get("findings") or []
        if (
            report.get("verdict") == "approve"
            and not any(finding.get("severity") in BLOCKING for finding in findings)
            and same_commit(reviewed_head(report), seen.head)
        ):
            return path
    return None


def done(board: Board, seen: Seen, step: dict[str, Any]) -> str | None:
    """The record that completes `step` by its kind's rule, or None while it is not done."""
    kind = step["done_by"]
    if kind == "plan":
        return plan_done(board, seen)
    if kind == "answer":
        return answer_done(board, seen)
    if kind == "check":
        return check_done(board, seen)
    if kind == "review":
        return review_done(board, seen, str(step["name"]))
    newest = seen.held.newest(LANDED_TYPE)
    return newest[0] if newest else None


def files_at(held: Records, head: str | None) -> list[str] | None:
    """The paths the files record at the head lists, or None without one."""
    for path, body in reversed(held.of(FILES_TYPE)):
        if same_commit(path.rsplit("/", 1)[-1], head):
            return [str(file) for file in body["files"]]
    return None


def added(board: Board, change: Change, files: list[str] | None) -> list[tuple[str, str]]:
    """Each (control, step) a control on the repository adds: those matching `files`, or every one without them."""
    # Only the steps controls add matter here, so an over-budget file never stops `next` or `brief`.
    given: dict[str, Any] = {"path": change["repo"], "limit": CONTROLS_LIMIT, "measure": False}
    if files is not None:
        given["files"] = files
    report = board.document("call", "controls.check", stdin=json.dumps(given).encode())
    output = cast("dict[str, Any]", report["data"]["output"])
    items: list[dict[str, Any]] = [*output["measured"], *output["stated"]]
    return [(str(item["control"]), str(item["adds_step"])) for item in items if item.get("adds_step")]


def planned(board: Board, change: Change, held: Records, head: str | None) -> list[dict[str, Any]]:
    """The change's steps in order, each with `added_by` and `if_touched`, before any is checked."""
    workflow = steps_of(board, change["workflow"])
    if workflow is None:
        raise PluginError(
            "changes::workflow::not_found",
            f"{change['workflow']}, the workflow of this change, holds no readable changes/workflow/v1; restore it,"
            " or run `board --personal call changes.seed` to write the seed workflows",
            {"workflow": change["workflow"]},
        )
    steps: list[dict[str, Any]] = [{**step, "added_by": None, "if_touched": False} for step in workflow]
    approves = change["route"] == "ask-first" or (change["route"] == "unread" and change["workflow"] != PERSONAL)
    if approves and not any(step["done_by"] == "answer" for step in steps):
        after = next((index + 1 for index, step in enumerate(steps) if step["done_by"] == "plan"), 0)
        steps.insert(after, {**APPROVE, "added_by": "route", "if_touched": False})
    files = files_at(held, head)
    for control, name in added(board, change, files):
        if any(step["name"] == name for step in steps):
            continue
        reviewers = [index for index, step in enumerate(steps) if step["actor"] == "reviewer"]
        landing = [index for index, step in enumerate(steps) if step["done_by"] == "landed"]
        at = reviewers[-1] + 1 if reviewers else (landing[0] if landing else len(steps))
        step = {"name": name, "actor": "reviewer", "done_by": "review", "added_by": control}
        steps.insert(at, step | {"if_touched": files is None})
    return steps


def derive(board: Board, path: str) -> dict[str, object]:
    """The change's head, each step done or not with the record that did it, the next step and its actor."""
    change = read(board, path)
    held = records(board, path)
    head = head_of(board, change, held)
    seen = Seen(path, change, held, head, citing(board, [path], "/for", (QUESTION_TYPE, BRIEF_TYPE)))
    states: list[dict[str, Any]] = []
    for step in planned(board, change, held, head):
        record = done(board, seen, step)
        states.append(step | {"done": record is not None, "record": record})
    ended = held.ended()
    following = None if ended else next((state for state in states if not state["done"]), None)
    return {
        "path": path,
        "head": head,
        "steps": states,
        "next": None if following is None else following["name"],
        "actor": None if following is None else following["actor"],
        "ended": ended,
    }


def next_step(request: CallRequest) -> dict[str, object]:
    """The `next` capability: where the change stands."""
    return derive(Board(request.board.root, PLUGIN), str(request.input["path"]).rstrip("/"))
