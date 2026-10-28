"""The change record: its type, its phases, and reading and writing it through Board."""

import json
from typing import Any, Required, TypedDict, cast

from board_plugin.board import Board, literal
from board_plugin.protocol import PluginError

PLUGIN = "changes"
CHANGE_TYPE = "changes/change/v1"
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
        "repo": Required[str],
        "phase": Required[str],
        "route": str,
        "plan": str,
        "issue": str,
        "head": str,
        "pull_request": str,
        "records": list[str],
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


def write(board: Board, lines: list[dict[str, Any]]) -> None:
    """Write each line of a `board put --batch` in one commit, or nothing."""
    board.document("put", "--batch", stdin="".join(json.dumps(line) + "\n" for line in lines).encode())
