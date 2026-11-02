"""The `open` capability: the changes that are not done or dropped, and, by kind, every prototype."""

import json
from typing import cast

from board_plugin.board import Board, literal
from board_plugin.protocol import CallRequest

from board_changes.change import CHANGE_TYPE, FINAL, PLUGIN, TREE_CODES, Change, branch_of, kind_of

# The default `limit` (STANDARDS 23).
OPEN_LIMIT = 100


def listed(path: str, change: Change) -> dict[str, object]:
    """One change as `open` lists it; a prototype also says what it asks, what it found and its branch."""
    kind = kind_of(change)
    shown: dict[str, object] = {
        "path": path,
        "title": change["title"],
        "mount": change["mount"],
        "phase": change["phase"],
        "route": change.get("route"),
        "task": change.get("task"),
        "kind": kind,
        "prototype": None,
    }
    if kind == "prototype":
        fields = ("question", "state", "verdict", "verdict_artifact")
        shown["prototype"] = {name: change.get(name) for name in fields} | {"branch": branch_of(path)}
    return shown


def open_changes(request: CallRequest) -> dict[str, object]:
    """Every change whose phase is not final, by path, at most `limit`; with `kind`, only those of that kind.

    With `kind` prototype, a settled or abandoned prototype is listed too, since its verdict is what is looked for.
    """
    limit = int(cast("int", request.input.get("limit", OPEN_LIMIT)))
    kind = request.input.get("kind")
    finals = ", ".join(literal(phase) for phase in FINAL)
    unfinished = "" if kind == "prototype" else f" AND json_extract(body, '$.phase') NOT IN ({finals})"
    rows = Board(request.board.root, PLUGIN).rows(
        "SELECT logical, code, body FROM records"  # noqa: S608 - values are quoted by literal()
        f" WHERE type = {literal(CHANGE_TYPE)}{unfinished} ORDER BY logical"
    )
    found: list[dict[str, object]] = []
    unreadable = 0
    for path, code, body in rows:
        if code is not None and code not in TREE_CODES:
            unreadable += 1
            continue
        change = cast("Change", json.loads(body))
        if kind is None or kind_of(change) == kind:
            found.append(listed(path, change))
    return {"changes": found[:limit], "total": len(found), "truncated": len(found) > limit, "unreadable": unreadable}
