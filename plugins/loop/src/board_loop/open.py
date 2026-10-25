"""The `open` capability: the changes that are not done or dropped."""

import json
from typing import Any, cast

from board_plugin.board import Board, literal
from board_plugin.protocol import CallRequest

from board_loop.change import CHANGE_TYPE, FINAL, PLUGIN, TREE_CODES

# The default `limit` (STANDARDS 23).
OPEN_LIMIT = 100


def open_changes(request: CallRequest) -> dict[str, object]:
    """Every change whose phase is not final, by path, at most `limit`."""
    limit = int(cast("int", request.input.get("limit", OPEN_LIMIT)))
    finals = ", ".join(literal(phase) for phase in FINAL)
    rows = Board(request.board.root, PLUGIN).rows(
        "SELECT logical, code, body FROM records"  # noqa: S608 - values are quoted by literal()
        f" WHERE type = {literal(CHANGE_TYPE)} AND json_extract(body, '$.phase') NOT IN ({finals}) ORDER BY logical"
    )
    found: list[dict[str, object]] = []
    unreadable = 0
    for path, code, body in rows:
        if code is not None and code not in TREE_CODES:
            unreadable += 1
            continue
        change = cast("dict[str, Any]", json.loads(body))
        found.append(
            {
                "path": path,
                "title": change["title"],
                "repo": change["repo"],
                "phase": change["phase"],
                "route": change.get("route"),
            }
        )
    return {"changes": found[:limit], "total": len(found), "truncated": len(found) > limit, "unreadable": unreadable}
