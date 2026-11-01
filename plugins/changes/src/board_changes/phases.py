"""The `next`, `attach` and `advance` capabilities: where a change stands, what it cites, and moving it on."""

import json
from typing import Any, cast

from board_plugin.board import Board, literal
from board_plugin.protocol import CallRequest, PluginError

from board_changes.change import FINAL, PHASES, PLUGIN, TREE_CODES, Change, read, write
from board_changes.gates import Observed, gate
from board_changes.repo import check_suite

INSTRUCTIONS = {
    "plan": "Interview the owner and fill in the plan's Problem, Outcome and Out of scope sections. On the "
    "ask-first route, attach the owner's decisions/decision/v1 whose basis names the plan.",
    "build": "Build test-first. Attach each task, record the head commit with --head, run the check suite on it "
    "and attach the run.",
    "review": "Review the head commit with clean context and attach the review/report/v1. Fix blocking findings, "
    "record the new head and review again.",
    "land": "Draft the pull request with `board call changes.pr`, open it with the command it returns, and record its "
    "URL with --pull-request. Merging happens outside Board.",
    "done": "Nothing is left to do.",
    "dropped": "Nothing is left to do.",
}


# The phases whose gates read the records a change references.
READS_RECORDS = ("plan", "build", "review")


def referenced(board: Board, refs: list[str]) -> tuple[dict[str, tuple[str, dict[str, Any]]], dict[str, str]]:
    """Each entry `refs` reaches as (type, body), in their order, and each unreadable one with the code that says why.

    A file that is not a record is of type `file`; a path Board resolves to a
    directory is of type `directory`.
    """
    records: dict[str, tuple[str, dict[str, Any]]] = {}
    unreadable = dict.fromkeys(refs, "board::path::not_found")
    if not refs:
        return records, unreadable
    listed = ", ".join(literal(ref) for ref in refs)
    rows = board.rows(
        "SELECT e.logical, e.type, e.code, r.body FROM entries AS e"  # noqa: S608 - values are quoted by literal()
        " LEFT JOIN records AS r ON r.logical = e.logical AND r.blob = e.blob"
        f" WHERE e.logical IN ({listed})"
    )
    for logical, type_name, code, text in rows:
        if code is not None and code not in TREE_CODES:
            unreadable[logical] = str(code)
        else:
            body = cast("dict[str, Any]", json.loads(text)) if text is not None else {}
            records[logical] = (str(type_name or "file"), body)
            unreadable.pop(logical, None)
    unfound = [ref for ref, code in unreadable.items() if code == "board::path::not_found"]
    for ref, answer in zip(unfound, board.refs(unfound), strict=True):
        if answer is not None:
            records[ref] = ("directory", {})
            del unreadable[ref]
    return {ref: records[ref] for ref in refs if ref in records}, unreadable


def plan_text(board: Board, change: Change) -> str | None:
    """The text of the change's plan, or `None` when it names none or nothing is there."""
    if "plan" not in change:
        return None
    [text] = board.get([change["plan"]])
    return None if isinstance(text, PluginError) else text.decode(errors="replace")


def observe(board: Board, path: str, change: Change) -> Observed:
    """What the change's gate reads: references where the gate uses them, the plan in `plan`, docs in `build`."""
    phase = change["phase"]
    cited = change.get("records", []) if phase in READS_RECORDS else []
    records, unreadable = referenced(board, cited)
    plan = plan_text(board, change) if phase == "plan" else None
    suite = check_suite(board, change["mount"]) if phase == "build" else None
    return Observed(path=path, change=change, records=records, plan=plan, suite=suite, unreadable=unreadable)


def next_step(request: CallRequest) -> dict[str, object]:
    """The change's phase and route, its gate's checks, and the next instruction."""
    path = str(request.input["path"])
    board = Board(request.board.root, PLUGIN)
    change, _ = read(board, path)
    checks = gate(observe(board, path, change))
    phase = change["phase"]
    instruction = INSTRUCTIONS[phase]
    if phase not in FINAL and all(found["status"] == "pass" for found in checks):
        instruction = f"The gate passes: run `board call changes.advance --path {path}`."
    return {"path": path, "phase": phase, "route": change.get("route"), "gate": checks, "instruction": instruction}


def attach(request: CallRequest) -> dict[str, object]:
    """Add a record reference, or set the route, head commit, issue or pull request, in one write."""
    asked = request.input
    path = str(asked["path"])
    board = Board(request.board.root, PLUGIN)
    change, blob = read(board, path)
    ref = asked.get("ref")
    if isinstance(ref, str):
        [answer] = board.refs([ref])
        if answer is None:
            raise PluginError(
                "changes::ref::dangling",
                f"nothing is at {ref}, so it cannot be attached; check the path with `board ls`",
                {"ref": ref},
            )
        records = change.setdefault("records", [])
        if ref not in records:
            records.append(ref)
    for field in ("route", "head", "issue", "pull_request"):
        value = asked.get(field)
        if isinstance(value, str):
            change[field] = value
    write(board, [{"path": path, "expect": blob, "record": change}])
    return {"path": path, "records": len(change.get("records", []))}


def advance(request: CallRequest) -> dict[str, object]:
    """Move the change to its next phase when its gate passes, or refuse and list what is missing."""
    path = str(request.input["path"])
    board = Board(request.board.root, PLUGIN)
    change, blob = read(board, path)
    phase = change["phase"]
    if phase in FINAL:
        raise PluginError(
            "changes::phase::final",
            f"the change at {path} is {phase}, which has no next phase; list the open changes with "
            "`board call changes.open`, or start one with `board call changes.start`",
            {"path": path, "phase": phase},
        )
    missing = [found for found in gate(observe(board, path, change)) if found["status"] == "missing"]
    if missing:
        raise PluginError(
            "changes::gate::unmet",
            f"the {phase} gate of {path} is unmet: "
            + "; ".join(f"{found['check']}: {found['detail']}" for found in missing)
            + f"; run `board call changes.next --path {path}` for the next step",
            {"path": path, "phase": phase, "missing": [found["check"] for found in missing]},
        )
    change["phase"] = PHASES[PHASES.index(phase) + 1]
    write(board, [{"path": path, "expect": blob, "record": change}])
    return {"path": path, "from": phase, "to": change["phase"]}
