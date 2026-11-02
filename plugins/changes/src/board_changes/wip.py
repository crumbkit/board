"""The cap on work in progress: at most three build changes in build, and apart from them three prototypes exploring.

A paused change counts toward neither, so pausing one frees its place.
"""

import json
from typing import cast

from board_plugin.board import Board, literal
from board_plugin.protocol import PluginError

from board_changes.change import CHANGE_TYPE, TREE_CODES, Change, kind_of

# The most build changes in build at once, and, counted apart, the most prototypes exploring.
WIP_LIMIT = 3


def building(changes: list[Change], kind: str) -> list[Change]:
    """The changes of `kind` that hold a place: a build change in build, or a prototype in build and exploring."""
    return [
        change
        for change in changes
        if change["phase"] == "build"
        and kind_of(change) == kind
        and (kind != "prototype" or change.get("state") == "exploring")
    ]


def in_build(board: Board) -> dict[str, Change]:
    """Every change in build that Board does not mark degraded, by path."""
    rows = board.rows(
        "SELECT logical, code, body FROM records"  # noqa: S608 - values are quoted by literal()
        f" WHERE type = {literal(CHANGE_TYPE)} AND json_extract(body, '$.phase') = 'build' ORDER BY logical"
    )
    return {
        str(path): cast("Change", json.loads(body)) for path, code, body in rows if code is None or code in TREE_CODES
    }


def refuse_full(board: Board, kind: str, path: str) -> None:
    """Refuse moving the change at `path` into build with `changes::wip::full` when its kind's cap is reached."""
    others = {other: change for other, change in in_build(board).items() if other != path}
    held = building(list(others.values()), kind)
    if len(held) < WIP_LIMIT:
        return
    paths = [other for other, change in others.items() if any(change is one for one in held)]
    what = "build changes are in build" if kind == "build" else "prototypes are exploring"
    then = (
        "pause the lowest-ranked with `board call changes.pause --path <change>`, or wait for one to move on"
        if kind == "build"
        else "ask the owner for a verdict on one with `board call changes.verdict`"
    )
    raise PluginError(
        "changes::wip::full",
        f"{len(held)} {what}, the most at once ({', '.join(paths)}), so {path} waits; {then}",
        {"path": path, "kind": kind, "building": paths},
    )
