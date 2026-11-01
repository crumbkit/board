"""The change record and the records under it, read and written through Board."""

import json
from dataclasses import dataclass
from datetime import UTC, datetime
from typing import Any, NotRequired, Required, TypedDict, cast

from board_plugin.board import Board, literal
from board_plugin.protocol import PluginError

PLUGIN = "changes"
CHANGE_TYPE = "changes/change/v1"
PLAN_TYPE = "changes/plan/v1"
WORKFLOW_TYPE = "changes/workflow/v1"
CLAIM_TYPE = "changes/claim/v1"
CHECK_TYPE = "changes/check/v1"
FILES_TYPE = "changes/files/v1"
OPENED_TYPE = "changes/opened/v1"
LANDED_TYPE = "changes/landed/v1"
DROPPED_TYPE = "changes/dropped/v1"
SUITE_TYPE = "runs/suite/v1"
RUN_TYPE = "runs/run/v1"
# The records that end a change, by the name each has in the change's directory.
ENDS = {"landed": LANDED_TYPE, "dropped": DROPPED_TYPE}
# Codes that depend on the tree rather than the record. A record holding one is still a record to read.
TREE_CODES = ("board::ref::dangling", "board::ref::wrong_type")
READABLE = f"(code IS NULL OR code IN ({', '.join(literal(code) for code in TREE_CODES)}))"

Change = TypedDict(
    "Change",
    {
        "$type": Required[str],
        "title": Required[str],
        "repo": Required[str],
        "branch": Required[str],
        "issue": NotRequired[str],
        "workflow": Required[str],
        "suite": Required[str],
        "route": Required[str],
    },
)
# A record as (logical path, body).
Found = tuple[str, dict[str, Any]]


def stamp() -> str:
    """A sortable UTC time to the microsecond, which names a record written once."""
    return datetime.now(UTC).strftime("%Y%m%dT%H%M%S%fZ")


def read(board: Board, path: str) -> Change:
    """The change at the logical `path`.

    A path that holds no change is `changes::change::not_found`. A change
    Board marks degraded for anything but a broken reference is
    `changes::change::degraded`, since its fields cannot be trusted.
    """
    rows = board.rows(
        f"SELECT code, body FROM records WHERE logical = {literal(path)} AND type = {literal(CHANGE_TYPE)}"  # noqa: S608 - values are quoted by literal()
    )
    if not rows:
        raise PluginError(
            "changes::change::not_found",
            f"no {CHANGE_TYPE} record is at {path}; name a change by its logical path, such as /changes/<name>, "
            "and list the open ones with `board --personal call changes.open`",
            {"path": path},
        )
    [[code, body]] = rows
    if code is not None and code not in TREE_CODES:
        raise PluginError(
            "changes::change::degraded",
            f"the change at {path} is degraded with {code}; run `board check {path}` and fix it",
            {"path": path, "code": code},
        )
    return cast("Change", json.loads(body))


@dataclass(frozen=True)
class Records:
    """The readable records under a change's directory, by type, each list ordered by path, so oldest stamp first."""

    by_type: dict[str, list[Found]]

    def of(self, type_name: str) -> list[Found]:
        """The records of `type_name`, oldest first."""
        return self.by_type.get(type_name, [])

    def newest(self, type_name: str) -> Found | None:
        """The newest record of `type_name`, or None."""
        found = self.of(type_name)
        return found[-1] if found else None

    def ended(self) -> str | None:
        """`landed` or `dropped` when that record exists, `landed` first; None while the change is open."""
        return next((name for name, type_name in ENDS.items() if self.of(type_name)), None)


def records(board: Board, path: str) -> Records:
    """The readable records under the change at `path`, by type, ordered by their paths."""
    prefix = f"{path}/"
    rows = board.rows(
        "SELECT logical, type, body FROM records"  # noqa: S608 - values are quoted by literal()
        f" WHERE substr(logical, 1, {len(prefix)}) = {literal(prefix)} AND {READABLE} ORDER BY logical"
    )
    found: dict[str, list[Found]] = {}
    for logical, type_name, body in rows:
        found.setdefault(str(type_name), []).append((str(logical), cast("dict[str, Any]", json.loads(body))))
    return Records(found)


def live(board: Board, path: str) -> tuple[Change, Records]:
    """The change at `path` and its records, refusing a change that ended with `changes::change::ended`."""
    change = read(board, path)
    held = records(board, path)
    ended = held.ended()
    if ended is not None:
        raise PluginError(
            "changes::change::ended",
            f"the change at {path} has {ended}, so it takes no new record; list open changes with "
            "`board --personal call changes.open`, or start a new one with `changes.start`",
            {"path": path, "ended": ended},
        )
    return change, held


def write(board: Board, lines: list[dict[str, Any]]) -> None:
    """Write each line of a `board put --batch` in one commit, or nothing."""
    board.document("put", "--batch", stdin="".join(json.dumps(line) + "\n" for line in lines).encode())


# The fewest hex digits a commit is named by when compared with another.
SHORT_COMMIT = 7


def same_commit(one: object, other: object) -> bool:
    """Whether `one` names the commit `other` names, either possibly abbreviated to at least 7 digits."""
    if not isinstance(one, str) or not isinstance(other, str) or min(len(one), len(other)) < SHORT_COMMIT:
        return False
    return one.startswith(other) or other.startswith(one)


def referenced(board: Board, refs: list[str]) -> tuple[dict[str, tuple[str, dict[str, Any]]], dict[str, str]]:
    """Each entry `refs` reaches as (type, body), in their order, and each unreadable one with the code that says why.

    A file that is not a record is of type `file`; a path Board resolves to a
    directory is of type `directory`. A record Board marks degraded for
    anything but a broken reference of its own is unreadable.
    """
    found: dict[str, tuple[str, dict[str, Any]]] = {}
    unreadable = dict.fromkeys(refs, "board::path::not_found")
    if not refs:
        return found, unreadable
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
            found[logical] = (str(type_name or "file"), body)
            unreadable.pop(logical, None)
    unfound = [ref for ref, code in unreadable.items() if code == "board::path::not_found"]
    for ref, answer in zip(unfound, board.refs(unfound), strict=True):
        if answer is not None:
            found[ref] = ("directory", {})
            del unreadable[ref]
    return {ref: found[ref] for ref in refs if ref in found}, unreadable
