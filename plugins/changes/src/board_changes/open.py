"""The `open` capability: the changes that have not ended."""

import json
from typing import Any, cast

from board_plugin.board import Board, literal
from board_plugin.protocol import CallRequest

from board_changes.change import CHANGE_TYPE, ENDS, PLUGIN, TREE_CODES

# The default `limit` (STANDARDS 23).
OPEN_LIMIT = 100


def open_changes(request: CallRequest) -> dict[str, object]:
    """Every change with no landed or dropped record beneath it, by path, at most `limit`."""
    limit = int(cast("int", request.input.get("limit", OPEN_LIMIT)))
    board = Board(request.board.root, PLUGIN)
    rows = board.rows(
        f"SELECT logical, code, body FROM records WHERE type = {literal(CHANGE_TYPE)} ORDER BY logical"  # noqa: S608 - values are quoted by literal()
    )
    ends = ", ".join(literal(kind) for kind in ENDS.values())
    ended = {
        str(row[0]).rsplit("/", 1)[0]
        for row in board.rows(f"SELECT logical FROM records WHERE type IN ({ends})")  # noqa: S608 - values are quoted by literal()
    }
    found: list[dict[str, object]] = []
    unreadable = 0
    for path, code, body in rows:
        if path in ended:
            continue
        if code is not None and code not in TREE_CODES:
            unreadable += 1
            continue
        change = cast("dict[str, Any]", json.loads(body))
        found.append({key: change[key] for key in ("title", "repo", "branch", "route")} | {"path": path})
    return {"changes": found[:limit], "total": len(found), "truncated": len(found) > limit, "unreadable": unreadable}
