"""What the controls plugin reads from the board: its controls and where paths lead."""

from typing import Any

from board_plugin.board import LIMIT, Board, literal
from board_plugin.protocol import PluginError
from pydantic import ValidationError

from board_controls.models import CONTROL_TYPE, ControlRecord


def controls(board: Board) -> dict[str, ControlRecord]:
    """Every control in the board's own tree this plugin can read, by its logical path.

    That includes a control Board marks degraded only because of the tree,
    such as one with a dangling reference; an invalid control is left out.
    """
    rows = board.rows(
        "SELECT logical, body, status FROM records"  # noqa: S608 - the one value is a constant
        f" WHERE type = {literal(CONTROL_TYPE)} AND source = 'worktree'",
    )
    found: dict[str, ControlRecord] = {}
    for logical, body, status in rows:
        try:
            found[logical] = ControlRecord.model_validate_json(body)
        except ValidationError as error:
            if status != "ok":
                continue
            raise PluginError(
                "controls::control::unreadable",
                f"{logical} is valid in the board but not a control this plugin reads: {error}",
                {"control": logical},
            ) from None
    return found


def degraded(board: Board) -> list[list[str]]:
    """Each control in the board's own tree that Board marks degraded, with its code, by path."""
    return board.rows(
        "SELECT logical, code FROM records"  # noqa: S608 - the one value is a constant
        f" WHERE type = {literal(CONTROL_TYPE)} AND status != 'ok' AND source = 'worktree' ORDER BY logical",
    )


def matched(board: Board, page: int = LIMIT) -> list[list[Any]]:
    """Each control and each file one of its patterns matches, with the file's status and code.

    A pointer's file holds where it leads, never what a budget counts, so no
    pattern matches one. The rows come in pages of at most `page`, ordered by
    the control's path and then the file's; each page asks for the rows after
    the last pair the page before it ended on. A file with no logical path
    is named by its path in the tree.
    """
    # Pages are keyed by path: unique in the working tree and, unlike logical, never null.
    after = ("", "")
    found: list[list[Any]] = []
    while True:
        rows, truncated = board.query(
            "SELECT DISTINCT r.logical, COALESCE(e.logical, e.path), e.status, e.code, r.path, e.path"  # noqa: S608 - values are quoted by literal()
            " FROM records r, json_each(r.body, '$.paths') p, entries e"
            f" WHERE r.type = {literal(CONTROL_TYPE)} AND r.status = 'ok' AND r.source = 'worktree'"
            " AND e.source = 'worktree' AND e.kind != 'pointer' AND e.path GLOB p.value"
            f" AND (r.path, e.path) > ({literal(after[0])}, {literal(after[1])})"
            " ORDER BY r.path, e.path",
            page,
        )
        found.extend(row[:4] for row in rows)
        if not truncated:
            return found
        if not rows or (rows[-1][4], rows[-1][5]) <= after:
            raise PluginError(
                "controls::board::failed",
                f"`board query` left rows out but returned none after ({after[0]}, {after[1]});"
                " check that the `board` first on PATH is this Board",
            )
        after = (rows[-1][4], rows[-1][5])


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
