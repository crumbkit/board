"""The `open` capability: the questions still waiting for the owner, oldest first."""

import datetime as dt
import json
from typing import Any, TypedDict, cast

from board_plugin.board import Board, literal
from board_plugin.protocol import CallRequest

QUESTION_TYPE = "plans/question/v1"
# The default `limit` (STANDARDS 23).
OPEN_LIMIT = 100
# Codes that depend on the tree rather than the record. A question holding one is still a question to read.
TREE_CODES = ("board::ref::dangling", "board::ref::wrong_type")


class OpenQuestion(TypedDict):
    """An open question, by path, with what the owner needs to answer it."""

    path: str
    question: str
    asked: str
    session: str
    options: list[dict[str, str]]
    recommendation: str | None
    blocks: str | None


def open_questions(request: CallRequest) -> dict[str, object]:
    """Every question below the path whose `status` is `open`, oldest first, at most `limit`.

    Board has validated the input against the manifest's schema.
    """
    asked = cast("dict[str, Any]", request.input)
    prefix = str(asked.get("path", "/")).rstrip("/") + "/"
    limit = int(asked.get("limit", OPEN_LIMIT))
    rows = Board(request.board.root, "plans").rows(
        "SELECT logical, code, body FROM records"  # noqa: S608 - values are quoted by literal()
        f" WHERE type = {literal(QUESTION_TYPE)} AND json_extract(body, '$.status') = 'open'"
        f" AND substr(logical, 1, length({literal(prefix)})) = {literal(prefix)}",
    )
    found: list[tuple[dt.datetime, OpenQuestion]] = []
    unreadable = 0
    for path, code, body in rows:
        if code is not None and code not in TREE_CODES:
            unreadable += 1
            continue
        # A mounted board validates against its own schemas, and Python reads fewer
        # RFC 3339 times than Board admits, such as a leap second, so a record may still fail here.
        try:
            record = cast("dict[str, Any]", json.loads(body))
            when = dt.datetime.fromisoformat(record["asked"].upper())
            entry = OpenQuestion(
                path=path,
                question=record["question"],
                asked=record["asked"],
                session=record["session"],
                options=record.get("options", []),
                recommendation=record.get("recommendation"),
                blocks=record.get("blocks"),
            )
        except (KeyError, TypeError, ValueError, AttributeError):
            unreadable += 1
            continue
        found.append((when, entry))
    found.sort(key=lambda pair: (pair[0], pair[1]["path"]))
    listed = [entry for _, entry in found]
    return {
        "questions": listed[:limit],
        "total": len(listed),
        "truncated": len(listed) > limit,
        "unreadable": unreadable,
    }
