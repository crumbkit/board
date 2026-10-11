"""The `next` capability: unfinished tasks in the order to do them, and what blocks each."""

import json
from typing import Any

from board_plugin.board import Board, literal
from board_plugin.protocol import CallRequest
from pydantic import ValidationError

from board_tasks.models import TASK_TYPE, UNFINISHED, NextInput, NextOutput, NextTask, TaskRecord

# Codes that depend on the tree rather than the record. A task holding one,
# such as an `after` that names no task yet, is still a task to list.
TREE_CODES = ("board::ref::dangling", "board::ref::wrong_type")

# Where a task is: the source that holds it, `worktree` or a commit id, and its physical path there.
Place = tuple[str, str]


def listed(values: tuple[str, ...]) -> str:
    """`values` as an SQL list of string literals."""
    return "(" + ", ".join(literal(value) for value in values) + ")"


def next_tasks(request: CallRequest) -> NextOutput:
    """Ask Board for the unfinished tasks and what blocks them.

    It runs one `board query`, and one `board refs` when a shown task waits.
    """
    limit = NextInput.model_validate(request.input).limit
    board = Board(request.board.root, "tasks")
    readable = f"type = {literal(TASK_TYPE)} AND (code IS NULL OR code IN {listed(TREE_CODES)})"
    status = "json_extract(body, '$.status')"
    # The first row holds the source and physical path of every `done` task, as one JSON array, so done
    # tasks do not count against the query's row limit. `records` gives a commit mounted at two paths only
    # one display path, so a task is matched by its source and physical path, which every route shares.
    # Every other row is an unfinished task.
    first, *rows = board.rows(
        "SELECT logical, body FROM ("  # noqa: S608 - every value is quoted by literal()
        "SELECT 0 AS kind, NULL AS logical, json_group_array(json_array(source, path)) AS body"
        f" FROM records WHERE {readable} AND {status} = 'done'"
        " UNION ALL SELECT 1, logical, body FROM records"
        f" WHERE {readable} AND logical IS NOT NULL AND {status} IN {listed(UNFINISHED)}"
        ") ORDER BY kind, json_extract(body, '$.due') IS NULL, json_extract(body, '$.due'), logical",
    )
    done: set[Place] = {(source, path) for source, path in json.loads(first[1])}
    tasks: list[tuple[str, TaskRecord]] = []
    for path, body in rows:
        try:
            tasks.append((path, TaskRecord.model_validate_json(body)))
        except ValidationError:
            # The board's own schema admits a record this plugin cannot read; `due` skips it too.
            continue
    shown = tasks[:limit]
    waiting = [path for path, task in shown if task.after]
    waits = {path: waiting_on(report) for path, report in zip(waiting, board.refs(waiting), strict=True)}
    blocking = {
        path: [display for display, place in targets.items() if place is None or place not in done]
        for path, targets in waits.items()
    }
    return NextOutput(
        tasks=[
            NextTask(
                path=path,
                title=task.title,
                status="waiting" if task.status == "waiting" else "open",
                due=task.due,
                blocked=bool(blocking.get(path)),
                blocked_by=blocking.get(path, []),
            )
            for path, task in shown
        ],
        total=len(tasks),
        truncated=len(tasks) > limit,
    )


def waiting_on(report: dict[str, Any] | None) -> dict[str, Place | None]:
    """Each task a task's `after` reaches, once, by its display path, and where it is, from its `board refs` report.

    A reference that reaches nothing is kept as written, with no place.
    """
    if report is None:
        return {}
    targets: dict[str, Place | None] = {}
    for outgoing in report["from"]:
        if not outgoing["pointer"].startswith("/after/"):
            continue
        location = outgoing["location"]
        if location is None:
            targets.setdefault(outgoing["target"], None)
        else:
            place = (location["source"], location["path"])
            targets.setdefault(location["display"], place)
    return targets
