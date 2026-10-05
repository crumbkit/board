"""What the controls plugin reads from the board: its controls and where paths lead."""

from typing import Any

from board_plugin.board import Board, literal
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


def referenced(board: Board, control: str) -> set[str]:
    """The display path of each entry the control's `applies_to` reaches, with every link replaced by its target.

    One `board refs` of the control resolves every reference it makes.
    """
    report = board.refs(control)
    if report is None:
        raise PluginError("controls::board::failed", f"nothing is at {control}")
    reached: set[str] = set()
    for outgoing in report["from"]:
        if outgoing["pointer"].startswith("/applies_to/"):
            if outgoing["location"] is None:
                raise PluginError("controls::board::failed", f"{control} reaches nothing at {outgoing['target']}")
            reached.add(outgoing["location"]["display"])
    return reached


def referencing(report: dict[str, Any]) -> set[str]:
    """The records in a `board refs` report whose `applies_to` reaches its path."""
    return {
        incoming["logical"]
        for incoming in report["to"]
        if incoming["logical"] and any(pointer.startswith("/applies_to/") for pointer in incoming["pointers"])
    }
