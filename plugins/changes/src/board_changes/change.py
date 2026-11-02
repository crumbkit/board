"""The change record: its type, its phases, and reading and writing it through Board."""

import json
from typing import Any, Required, TypedDict, cast

from board_plugin.board import Board, literal
from board_plugin.protocol import PluginError

PLUGIN = "changes"
CHANGE_TYPE = "changes/change/v1"
PLAN_TYPE = "changes/plan/v1"
# The record types a change's `tickets` and `pulls` may reference; the jira and github plugins own them.
TICKET_TYPES = ("jira/issue/v1", "github/issue/v1")
PULL_TYPES = ("github/pull/v1",)
# The record type a change's `task` references; the tasks plugin owns it.
TASK_TYPE = "tasks/task/v2"
# What a change is for: `build` lands on main, `prototype` answers a question on its own branch and never lands.
KINDS = ("build", "prototype")
# Where a prototype stands: exploring until its verdict settles or abandons it.
STATES = ("exploring", "settled", "abandoned")
# The phases a change passes through, in order. `dropped` ends a change outside this order.
PHASES = ("plan", "build", "review", "land", "done")
FINAL = ("done", "dropped")
# Codes that depend on the tree rather than the record. A record holding one is still a record to read.
TREE_CODES = ("board::ref::dangling", "board::ref::wrong_type")

Change = TypedDict(
    "Change",
    {
        "$type": Required[str],
        "title": Required[str],
        "mount": Required[str],
        "phase": Required[str],
        "route": str,
        "plan": str,
        "issue": str,
        "head": str,
        "pull_request": str,
        "tickets": list[str],
        "pulls": list[str],
        "records": list[str],
        "task": str,
        "kind": str,
        "question": str,
        "state": str,
        "verdict": str,
        "verdict_artifact": str,
    },
    total=False,
)


def read(board: Board, path: str) -> tuple[Change, str]:
    """The change at the logical `path` and the blob that holds it.

    A path that holds no change is `changes::change::not_found`. A change
    Board marks degraded for anything but a broken reference is
    `changes::change::degraded`, since its fields cannot be trusted.
    """
    rows = board.rows(
        f"SELECT blob, code, body FROM records WHERE logical = {literal(path)} AND type = {literal(CHANGE_TYPE)}"  # noqa: S608 - values are quoted by literal()
    )
    if not rows:
        raise PluginError(
            "changes::change::not_found",
            f"no {CHANGE_TYPE} record is at {path}; name a change by its logical path, such as /changes/<name>, "
            "and list the open ones with `board call changes.open`",
            {"path": path},
        )
    [[blob, code, body]] = rows
    if code is not None and code not in TREE_CODES:
        raise PluginError(
            "changes::change::degraded",
            f"the change at {path} is degraded with {code}; run `board check {path}` and fix it",
            {"path": path, "code": code},
        )
    return cast("Change", json.loads(body)), str(blob)


def kind_of(change: Change) -> str:
    """The change's kind; `build` when it names none, as a change written before kinds does."""
    return change.get("kind", "build")


def name_in(path: str) -> str:
    """The change's name, the last part of its logical path, such as `fix-it` in `/changes/fix-it`."""
    return path.rstrip("/").rsplit("/", 1)[-1]


def branch_of(path: str) -> str:
    """The only branch a prototype's builder may edit: `proto/<name>`."""
    return f"proto/{name_in(path)}"


def write(board: Board, lines: list[dict[str, Any]]) -> None:
    """Write each line of a `board put --batch` in one commit, or nothing."""
    board.document("put", "--batch", stdin="".join(json.dumps(line) + "\n" for line in lines).encode())
