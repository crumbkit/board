"""The `open` capability: the questions still waiting for the owner, oldest first."""

import datetime as dt
import json
import re
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


# An RFC 3339 time, which always parses with an offset.
TIME = re.compile(r"\d{4}-\d{2}-\d{2}[Tt]\d{2}:\d{2}:\d{2}(\.\d+)?([Zz]|[+-]\d{2}:\d{2})")


def readable(path: str, body: str) -> tuple[dt.datetime, OpenQuestion] | None:
    """The question in `body` and when it was asked, or None when it does not have the shape `open` returns.

    Board has validated the record, but a mounted board validates against its own schemas, and Python reads
    fewer RFC 3339 times than Board admits, such as a leap second, so the shape is checked here.
    """
    record = json.loads(body)
    if not isinstance(record, dict):
        return None
    record = cast("dict[str, Any]", record)
    asked, options = record.get("asked"), cast("object", record.get("options", []))
    texts = [record.get("question"), record.get("session")]
    optional = [record.get("recommendation"), record.get("blocks")]
    if not (
        isinstance(asked, str)
        and TIME.fullmatch(asked)
        and all(isinstance(text, str) for text in texts)
        and all(text is None or isinstance(text, str) for text in optional)
        and isinstance(options, list)
        and all(
            isinstance(option, dict) and option_ok(cast("dict[str, Any]", option))
            for option in cast("list[object]", options)
        )
    ):
        return None
    try:
        when = dt.datetime.fromisoformat(asked.upper())
    except ValueError:
        return None
    question = OpenQuestion(
        path=path,
        question=record["question"],
        asked=asked,
        session=record["session"],
        options=cast("list[dict[str, str]]", options),
        recommendation=record.get("recommendation"),
        blocks=record.get("blocks"),
    )
    return when, question


def option_ok(option: dict[str, Any]) -> bool:
    """Whether an option has the non-empty string `id` and `text` that `open` returns, and nothing else."""
    return set(option) == {"id", "text"} and all(isinstance(value, str) and value for value in option.values())


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
        read = readable(path, body) if code is None or code in TREE_CODES else None
        if read is None:
            unreadable += 1
        else:
            found.append(read)
    found.sort(key=lambda pair: (pair[0], pair[1]["path"]))
    listed = [entry for _, entry in found]
    return {
        "questions": listed[:limit],
        "total": len(listed),
        "truncated": len(listed) > limit,
        "unreadable": unreadable,
    }
