"""The `next` capability: unfinished tasks in the order to do them, and what blocks each."""

from typing import Any

from board_plugin.board import Board, literal
from board_plugin.protocol import CallRequest
from pydantic import ValidationError

from board_tasks.models import FIELDS, TASK_TYPE, TREE_CODES, UNFINISHED, NextInput, NextOutput, NextTask, TaskRecord


def listed(values: tuple[str, ...]) -> str:
    """`values` as an SQL list of string literals."""
    return "(" + ", ".join(literal(value) for value in values) + ")"


def next_tasks(request: CallRequest) -> NextOutput:
    """Ask Board for the unfinished tasks and what blocks them.

    It runs one `board query` for the unfinished tasks, and one `board refs` when a shown task waits, which
    reads the status of each task it waits on. Both are bounded by what is shown, not by how many tasks are done.
    """
    limit = NextInput.model_validate(request.input).limit
    board = Board(request.board.root, "tasks")
    readable = f"type = {literal(TASK_TYPE)} AND (code IS NULL OR code IN {listed(TREE_CODES)})"
    status = "json_extract(body, '$.status')"
    rows = board.rows(
        "SELECT logical, body FROM records"  # noqa: S608 - every value is quoted by literal()
        f" WHERE {readable} AND logical IS NOT NULL AND {status} IN {listed(UNFINISHED)}"
        " ORDER BY json_extract(body, '$.due') IS NULL, json_extract(body, '$.due'), logical",
    )
    tasks: list[tuple[str, TaskRecord]] = []
    for path, body in rows:
        try:
            tasks.append((path, TaskRecord.model_validate_json(body)))
        except ValidationError:
            # The board's own schema admits a record this plugin cannot read; `due` skips it too.
            continue
    shown = tasks[:limit]
    waiting = [path for path, task in shown if task.after]
    reports = board.refs(waiting, fields=FIELDS)
    waits = {path: waiting_on(report) for path, report in zip(waiting, reports, strict=True)}
    blocking = {
        path: [display for display, finished in targets.items() if not finished] for path, targets in waits.items()
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


def waiting_on(report: dict[str, Any] | None) -> dict[str, bool]:
    """Each task a task's `after` reaches, once, by its display path, and whether it is done, from its refs report.

    A reference that reaches nothing is kept as written, and is not done.
    """
    if report is None:
        return {}
    targets: dict[str, bool] = {}
    for outgoing in report["from"]:
        if not outgoing["pointer"].startswith("/after/"):
            continue
        location = outgoing["location"]
        display = outgoing["target"] if location is None else location["display"]
        targets.setdefault(display, done(outgoing))
    return targets


def done(outgoing: dict[str, Any]) -> bool:
    """Whether a reference reaches a done task: a task, as `next` lists them, whose status is `done`.

    A target that check degrades for anything but the tree, or that is not a record, is not done.
    """
    values = {value["pointer"]: value.get("value") for value in outgoing.get("values", [])}
    code = outgoing.get("target_code")
    return (
        values.get("/$type") == TASK_TYPE and values.get("/status") == "done" and (code is None or code in TREE_CODES)
    )
