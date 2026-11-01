"""What the controls plugin reads from the board: its controls and where paths lead."""

import json
from dataclasses import dataclass
from typing import Any

from board_plugin.board import LIMIT, Board, literal
from board_plugin.protocol import PluginError

from board_controls.shapes import CONTROL_TYPE, DECISION_TYPE, ControlRecord

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

# How many named files one query compares with the controls' patterns, so the SQL stays well within
# what one command line may hold.
NAMED_PER_QUERY = 1_000


@dataclass(frozen=True)
class Own:
    """A mounted board whose own controls a check reads: the commit Board reads it at, and its display path."""

    source: str
    root: str


@dataclass(frozen=True)
class Control:
    """A control, the root of the board that holds it, `/` for this board's own tree, and Board's source of it."""

    record: ControlRecord
    board: str
    source: str


def _sources(own: Own | None) -> str:
    """The SQL condition on `source` that selects the controls a read considers."""
    if own is None:
        return "source = 'worktree'"
    return f"source IN ('worktree', {literal(own.source)})"


def patterns(own: Own | None = None) -> str:
    """The CTE of each pattern of each control, with the `anchor` its pattern is matched beneath.

    It holds controls Board marks degraded too; callers keep only those `controls` returns.

    A pattern matches the part of a file's `full` path after its anchor. The
    anchor is the root of the control's board, empty for this board's own
    tree and the mount's path for `own`'s, followed by each repository in
    `repos`, as written, without its leading `/`. MATERIALIZED parses each
    control's body once, not once for every file a pattern is compared with.
    """
    base = "''" if own is None else f"CASE r.source WHEN 'worktree' THEN '' ELSE {literal(own.root[1:] + '/')} END"
    return (
        "patterns AS MATERIALIZED (SELECT r.logical, p.value AS pattern,"  # noqa: S608 - values are quoted by literal()
        f" {base} || COALESCE(substr(q.value, 2) || '/', '') AS anchor"
        " FROM records r JOIN json_each(r.body, '$.paths') p LEFT JOIN json_each(r.body, '$.repos') q"
        f" WHERE r.type = {literal(CONTROL_TYPE)} AND r.{_sources(own)})"
    )


def match(full: str) -> str:
    """The SQL condition that the path `full` lies beneath a pattern's anchor and matches the pattern there."""
    return f"substr({full}, 1, length(c.anchor)) = c.anchor AND substr({full}, length(c.anchor) + 1) GLOB c.pattern"


def controls(board: Board, own: Own | None = None) -> dict[str, Control]:
    """Every valid control in the board's own tree, and in `own`'s board, by its logical path.

    That includes a control Board marks degraded only because a reference
    dangles or reaches the wrong type, which Board checks only of a valid
    record. A malformed reference fails the schema, so it is left out.
    """
    rows = board.rows(
        "SELECT logical, body, source FROM records"  # noqa: S608 - values are quoted by literal()
        f" WHERE type = {literal(CONTROL_TYPE)} AND {_sources(own)}"
        " AND (status = 'ok' OR code IN ('board::ref::dangling', 'board::ref::wrong_type'))",
    )
    return {
        logical: Control(json.loads(body), "/" if own is None or source == "worktree" else own.root, source)
        for logical, body, source in rows
    }


def degraded(board: Board, own: Own | None = None) -> list[list[str]]:
    """Each control in the board's own tree, and in `own`'s board, that Board marks degraded, with its code."""
    return board.rows(
        "SELECT logical, code FROM records"  # noqa: S608 - values are quoted by literal()
        f" WHERE type = {literal(CONTROL_TYPE)} AND status != 'ok' AND {_sources(own)} ORDER BY logical",
    )


def superseded(board: Board) -> set[tuple[str, str]]:
    """Each decision some decision lists in `supersedes`, as written, with the source of the board both are in."""
    rows = board.rows(
        "SELECT DISTINCT r.source, s.value FROM records r, json_each(r.body, '$.supersedes') s"  # noqa: S608
        f" WHERE r.type = {literal(DECISION_TYPE)}",
    )
    return {(source, value) for source, value in rows}


def matched(board: Board, own: Own | None = None, page: int = LIMIT) -> list[list[Any]]:
    """Each control and each file one of its patterns matches, with the file's status and code.

    A pattern matches a file's `full` path (see FILES) beneath its anchor (see
    `patterns`), so files in mounted boards match by their display path
    through the mount. A pointer's file
    holds where it leads, never what a budget counts, so no pattern matches
    one. The rows come in
    pages of at most `page`, ordered by the control's path and then the
    file's; each page asks for the rows after the last pair the page before
    it ended on.
    """
    # Pages are keyed by the control, the matched path and the file's source, which together are unique.
    after = ("", "", "")
    found: list[list[Any]] = []
    while True:
        rows, truncated = board.query(matching(after, own), page)
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


def matching(after: tuple[str, str, str], own: Own | None = None) -> str:
    """The SQL for one page of `matched`: the pairs after `after`, a control's path and a file's `full` and source."""
    return (
        f"WITH files AS MATERIALIZED {FILES}, {patterns(own)} SELECT DISTINCT"  # noqa: S608 - values are quoted by literal()
        " c.logical, COALESCE(e.logical, e.full), e.status, e.code, c.logical, e.full, e.source"
        f" FROM patterns c, files e WHERE {match('e.full')}"
        f" AND (c.logical, e.full, e.source) > ({', '.join(literal(value) for value in after)})"
        " ORDER BY c.logical, e.full, e.source"
    )


def named(board: Board, files: list[str], own: Own | None = None) -> set[tuple[str, str]]:
    """Each control and each of `files`, display paths, that one of its patterns matches.

    The files need not be in the board, so a repository it does not mount is
    matched through the files a caller names.
    """
    found: set[tuple[str, str]] = set()
    for start in range(0, len(files), NAMED_PER_QUERY):
        values = ", ".join(f"({literal(file[1:])})" for file in files[start : start + NAMED_PER_QUERY])
        rows = board.rows(
            f"WITH named(full) AS (VALUES {values}), {patterns(own)}"  # noqa: S608 - values are quoted by literal()
            f" SELECT DISTINCT c.logical, n.full FROM patterns c, named n WHERE {match('n.full')}"
        )
        found.update((control, "/" + full) for control, full in rows)
    return found


def referenced(board: Board, controls: dict[str, Control]) -> dict[str, set[str]]:
    """For each control with `applies_to`, the display path of each entry it reaches.

    Every link is replaced by its target. A reference in a mounted board is
    read in that board, so its display path is put beneath the mount's. One
    `board refs` resolves every reference the controls make, and no controls
    start no run.
    """
    asked = sorted(logical for logical, control in controls.items() if control.record.get("applies_to"))
    found: dict[str, set[str]] = {}
    for control, report in zip(asked, board.refs(asked), strict=True):
        if report is None:
            raise PluginError("controls::board::failed", f"nothing is at {control}")
        reached: set[str] = set()
        for outgoing in report["from"]:
            if outgoing["pointer"].startswith("/applies_to/"):
                if outgoing["location"] is None:
                    raise PluginError("controls::board::failed", f"{control} reaches nothing at {outgoing['target']}")
                display = str(outgoing["location"]["display"])
                root = controls[control].board
                reached.add(display if root == "/" else root + display.rstrip("/"))
        found[control] = reached
    return found


def referencing(report: dict[str, Any]) -> set[str]:
    """The records in a `board refs` report whose `applies_to` reaches its path."""
    return {
        incoming["logical"]
        for incoming in report["to"]
        if incoming["logical"] and any(pointer.startswith("/applies_to/") for pointer in incoming["pointers"])
    }
