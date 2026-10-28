"""A plugin's migrate capability: rewrite its records of types it no longer ships in one `board put --batch`.

`board plugin enable` keeps the schema of a type the plugin no longer ships while a record in the board's own
tree has that type, so the record stays valid and the plugin still runs. The capability reads those records
with `olds`, builds lines with `put`, `move` and `remove`, and passes them to `migrate`. Each line expects the
blob it read, so a file changed since the read refuses the whole batch with `board::put::stale`.
"""

import json
from collections.abc import Mapping, Sequence
from dataclasses import dataclass
from typing import Any, cast

from board_plugin.board import Board, literal

# The most lines the output lists (STANDARDS 23).
LIST_LIMIT = 100

type Line = dict[str, object]


@dataclass(frozen=True)
class Old:
    """A record in the board's own tree, as read: its logical path, its blob and its value."""

    path: str
    blob: str
    record: dict[str, Any]


def olds(board: Board, types: Sequence[str]) -> list[Old]:
    """Every record of one of `types` in the board's own tree that parses, by logical path."""
    if not types:
        return []
    listed = ", ".join(literal(name) for name in types)
    rows = board.rows(
        "SELECT logical, blob, body FROM records WHERE source = 'worktree'"  # noqa: S608 - literals quoted
        f" AND type IN ({listed}) AND body IS NOT NULL ORDER BY logical"
    )
    return [Old(str(path), str(blob), cast("dict[str, Any]", json.loads(body))) for path, blob, body in rows]


def put(old: Old, record: Mapping[str, object]) -> Line:
    """A line that replaces `old` with `record` at its path."""
    return {"path": old.path, "expect": old.blob, "record": dict(record)}


def move(old: Old, to: str, text: str) -> list[Line]:
    """The lines that write `text` to the new file `to` and remove `old`."""
    return [{"path": to, "expect": "none", "text": text}, remove(old)]


def remove(old: Old) -> Line:
    """A line that removes `old`."""
    return {"path": old.path, "expect": old.blob, "remove": True}


def migrate(board: Board, lines: Sequence[Line], owed: Sequence[str] = (), *, apply: bool) -> dict[str, object]:
    """The capability's output: the lines, what the owner owes afterwards, and the commit.

    Without `apply` it writes nothing. With it, one `board put --batch` writes every line, and `commit` is the
    commit holding them, or null when the board does not commit puts or there were no lines. `lines` lists at
    most 100; `total` counts them all, and `truncated` says whether some were left out.
    """
    commit = None
    if apply and lines:
        report = board.document("put", "--batch", stdin="".join(json.dumps(line) + "\n" for line in lines).encode())
        commit = cast("dict[str, Any]", report.get("data", {})).get("commit")
    return {
        "applied": apply,
        "commit": commit,
        "lines": list(lines[:LIST_LIMIT]),
        "total": len(lines),
        "truncated": len(lines) > LIST_LIMIT,
        "owed": list(owed),
    }
