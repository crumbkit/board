"""What the controls plugin reads from the board: its controls and where paths lead."""

import json
from typing import Any

from board_plugin.board import LIMIT, Board, literal
from board_plugin.protocol import PluginError

from board_controls.shapes import CONTROL_TYPE, ControlRecord

# Each file a pattern can match, with `full`: its display path without the
# leading `/`, with `.json` put back when the display path dropped it. A file
# in the own tree whose path is malformed has no display path and keeps its
# path, so a budget never leaves it out silently. A query names it with
# `files AS {FILES}`, and adds MATERIALIZED where it compares every file with
# every pattern, so each row is computed once. Without MATERIALIZED, a lookup
# of one file uses the index on its source and path.
FILES = (
    "(SELECT *, COALESCE(substr(logical, 2)"
    " || CASE WHEN substr(logical, -length(path) - 1) = '/' || path THEN '' ELSE '.json' END,"
    " CASE source WHEN 'worktree' THEN path END) AS full FROM entries WHERE kind != 'pointer')"
)

# Each pattern of each readable control in the board's own tree. MATERIALIZED
# parses each control's body once, not once for every file a pattern is
# compared with.
PATTERNS = (
    "patterns AS MATERIALIZED (SELECT r.logical, r.path, p.value AS pattern"  # noqa: S608 - the one value is a constant
    " FROM records r, json_each(r.body, '$.paths') p"
    f" WHERE r.type = {literal(CONTROL_TYPE)} AND r.status = 'ok' AND r.source = 'worktree')"
)


def controls(board: Board) -> dict[str, ControlRecord]:
    """Every valid control in the board's own tree, by its logical path.

    That includes a control Board marks degraded only because a reference
    dangles or reaches the wrong type, which Board checks only of a valid
    record. A malformed reference fails the schema, so it is left out.
    """
    rows = board.rows(
        "SELECT logical, body FROM records"  # noqa: S608 - the one value is a constant
        f" WHERE type = {literal(CONTROL_TYPE)} AND source = 'worktree'"
        " AND (status = 'ok' OR code IN ('board::ref::dangling', 'board::ref::wrong_type'))",
    )
    return {logical: json.loads(body) for logical, body in rows}


def degraded(board: Board) -> list[list[str]]:
    """Each control in the board's own tree that Board marks degraded, with its code, by path."""
    return board.rows(
        "SELECT logical, code FROM records"  # noqa: S608 - the one value is a constant
        f" WHERE type = {literal(CONTROL_TYPE)} AND status != 'ok' AND source = 'worktree' ORDER BY logical",
    )


def matched(board: Board, page: int = LIMIT) -> list[list[Any]]:
    """Each control and each file one of its patterns matches, with the file's status and code.

    A pattern matches a file's `full` path (see FILES), so files in mounted
    boards match by their display path through the mount. A pointer's file
    holds where it leads, never what a budget counts, so no pattern matches
    one. The rows come in
    pages of at most `page`, ordered by the control's path and then the
    file's; each page asks for the rows after the last pair the page before
    it ended on.
    """
    # Pages are keyed by the matched path and the file's source, which together are unique.
    after = ("", "", "")
    found: list[list[Any]] = []
    while True:
        rows, truncated = board.query(matching(after), page)
        found.extend(row[:4] for row in rows)
        if not truncated:
            return found
        if not rows or tuple(rows[-1][4:]) <= after:
            raise PluginError(
                "controls::board::failed",
                f"`board query` left rows out but returned none after ({', '.join(after)});"
                " check that the `board` first on PATH is this Board",
            )
        after = (rows[-1][4], rows[-1][5], rows[-1][6])


def matching(after: tuple[str, str, str]) -> str:
    """The SQL for one page of `matched`: the pairs after `after`, a control's path and a file's `full` and source."""
    return (
        f"WITH files AS MATERIALIZED {FILES}, {PATTERNS} SELECT DISTINCT"  # noqa: S608 - values are quoted by literal()
        " c.logical, COALESCE(e.logical, e.full), e.status, e.code, c.path, e.full, e.source"
        " FROM patterns c, files e WHERE e.full GLOB c.pattern"
        f" AND (c.path, e.full, e.source) > ({', '.join(literal(value) for value in after)})"
        " ORDER BY c.path, e.full, e.source"
    )


def referenced(board: Board, controls: list[str]) -> dict[str, set[str]]:
    """For each control, the display path of each entry its `applies_to` reaches.

    Every link is replaced by its target. One `board refs` resolves every
    reference the controls make, and no controls start no run.
    """
    found: dict[str, set[str]] = {}
    for control, report in zip(controls, board.refs(controls), strict=True):
        if report is None:
            raise PluginError("controls::board::failed", f"nothing is at {control}")
        reached: set[str] = set()
        for outgoing in report["from"]:
            if outgoing["pointer"].startswith("/applies_to/"):
                if outgoing["location"] is None:
                    raise PluginError("controls::board::failed", f"{control} reaches nothing at {outgoing['target']}")
                reached.add(outgoing["location"]["display"])
        found[control] = reached
    return found


def referencing(report: dict[str, Any]) -> set[str]:
    """The records in a `board refs` report whose `applies_to` reaches its path."""
    return {
        incoming["logical"]
        for incoming in report["to"]
        if incoming["logical"] and any(pointer.startswith("/applies_to/") for pointer in incoming["pointers"])
    }
