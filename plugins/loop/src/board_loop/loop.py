"""The `next`, `attach` and `advance` capabilities: where a change stands, what it cites, and moving it on."""

import json
from typing import Any, cast

from board_plugin.board import Board, literal
from board_plugin.protocol import CallRequest, PluginError

from board_loop.change import FINAL, PHASES, PLUGIN, TREE_CODES, Change, read, write
from board_loop.gates import Observed, gate
from board_loop.repo import check_suite

INSTRUCTIONS = {
    "shape": "Interview the owner and fill in the plan's Problem, Outcome and Out of scope sections. Record each "
    "question for the owner as a plans/question/v1 and attach it.",
    "plan": "Have the plan reviewed with clean context and attach the review/report/v1. On the ask-first route, "
    "attach the owner's plans/decision/v1 whose refs name the plan.",
    "build": "Build test-first. Attach each task, record the head commit with --head, run the check suite on it "
    "and attach the run.",
    "review": "Review the head commit with clean context and attach the review/report/v1. Fix blocking findings, "
    "record the new head and review again.",
    "land": "Open the pull request and record its URL with --pull-request. Merging happens outside Board.",
    "learn": "Record what to improve as notes or tasks and attach them, or decide there is nothing, then advance.",
    "done": "Nothing is left to do.",
    "dropped": "Nothing is left to do.",
}


# The phases whose gates read the records a change references.
READS_RECORDS = ("shape", "plan", "build", "review")


def observe(board: Board, path: str, change: Change) -> Observed:
    """What the change's gate reads: references where the gate uses them, the plan in `shape`, docs in `build`."""
    phase = change["phase"]
    cited = change.get("records", []) if phase in READS_RECORDS else []
    records: dict[str, tuple[str, dict[str, Any]]] = {}
    if cited:
        listed = ", ".join(literal(ref) for ref in cited)
        rows = board.rows(f"SELECT logical, type, code, body FROM records WHERE logical IN ({listed})")  # noqa: S608 - values are quoted by literal()
        found = {row[0]: row for row in rows if row[2] is None or row[2] in TREE_CODES}
        for ref in cited:
            if ref in found:
                _, type_name, _, body = found[ref]
                records[ref] = (str(type_name), cast("dict[str, Any]", json.loads(body)))
    plan = None
    if phase == "shape" and "plan" in change:
        [text] = board.get([change["plan"]])
        plan = None if isinstance(text, PluginError) else text.decode(errors="replace")
    suite = check_suite(board, change["repo"]) if phase == "build" else None
    return Observed(path=path, change=change, records=records, plan=plan, suite=suite)


def next_step(request: CallRequest) -> dict[str, object]:
    """The change's phase and route, its gate's checks, and the next instruction."""
    path = str(request.input["path"])
    board = Board(request.board.root, PLUGIN)
    change, _ = read(board, path)
    checks = gate(observe(board, path, change))
    phase = change["phase"]
    instruction = INSTRUCTIONS[phase]
    if phase not in FINAL and all(found["status"] == "pass" for found in checks):
        instruction = f"The gate passes: run `board call loop.advance --path {path}`."
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
                "loop::ref::dangling",
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
            "loop::phase::final",
            f"the change at {path} is {phase}, which has no next phase; list the open changes with "
            "`board call loop.open`, or start one with `board call loop.start`",
            {"path": path, "phase": phase},
        )
    missing = [found for found in gate(observe(board, path, change)) if found["status"] == "missing"]
    if missing:
        raise PluginError(
            "loop::gate::unmet",
            f"the {phase} gate of {path} is unmet: "
            + "; ".join(f"{found['check']}: {found['detail']}" for found in missing)
            + f"; run `board call loop.next --path {path}` for the next step",
            {"path": path, "phase": phase, "missing": [found["check"] for found in missing]},
        )
    change["phase"] = PHASES[PHASES.index(phase) + 1]
    write(board, [{"path": path, "expect": blob, "record": change}])
    return {"path": path, "from": phase, "to": change["phase"]}
