"""The change record: its type, its phases, and reading and writing it through Board."""

import json
from datetime import UTC, datetime
from typing import Any, Required, TypedDict, cast

from board_plugin.board import Board, literal
from board_plugin.protocol import PluginError

PLUGIN = "changes"
CHANGE_TYPE = "changes/change/v1"
PLAN_TYPE = "changes/plan/v1"
UNDERSTANDING_TYPE = "changes/understanding/v1"
SYNC_TYPE = "changes/sync/v1"
# Where `sync` records the day it last ran.
SYNC_PATH = "/changes/sync"
SYNC_COMMAND = "board call changes.sync"
# The board's own changes; a mounted board's changes sit under the mount and are that board's to keep.
OWN = "/changes/"
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
        "merge_commit_url": str,
        "dropped_reason": str,
        "over_limit": str,
        "touched": str,
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


def today() -> str:
    """Today's UTC day, as `YYYY-MM-DD`."""
    return datetime.now(UTC).date().isoformat()


def touch(change: Change) -> Change:
    """`change` with `touched` set to today, which keeps it active under the WIP limit."""
    change["touched"] = today()
    return change


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


def newest(board: Board, path: str) -> tuple[str | None, list[str], list[str]]:
    """The text path, `uses` and `paths` of the change's newest plan record, or no text, uses or paths."""
    prefix = f"{path}/plans/"
    rows = board.rows(
        f"SELECT body FROM records WHERE type = {literal(PLAN_TYPE)}"  # noqa: S608 - values are quoted by literal()
        f" AND substr(logical, 1, {len(prefix)}) = {literal(prefix)}"
        f" AND (code IS NULL OR code IN ({', '.join(literal(code) for code in TREE_CODES)}))"
        " ORDER BY logical DESC LIMIT 1"
    )
    if not rows:
        return None, [], []
    body = json.loads(rows[0][0])
    return str(body["text"]), [str(ref) for ref in body["uses"]], [str(each) for each in body.get("paths", [])]


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


def last_sync(board: Board) -> dict[str, object]:
    """The day the last writing sync ran and whether that is before today, with the warning and command when it is."""
    rows = board.rows(
        f"SELECT json_extract(body, '$.synced') FROM records WHERE logical = {literal(SYNC_PATH)}"  # noqa: S608 - quoted by literal()
        f" AND type = {literal(SYNC_TYPE)}"
    )
    synced = str(rows[0][0]) if rows and rows[0][0] is not None else None
    stale = synced is None or synced < today()
    return {
        "synced": synced,
        "stale": stale,
        "code": "changes::sync::stale" if stale else None,
        "command": SYNC_COMMAND,
    }
