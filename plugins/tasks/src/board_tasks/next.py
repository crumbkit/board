"""The `next` capability: unfinished tasks in the order to do them, and what blocks each."""

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
    """Ask Board, through `board query` and `board refs`, for the unfinished tasks and what blocks them."""
    limit = NextInput.model_validate(request.input).limit
    board = Board(request.board.root, "tasks")
    rows = board.rows(
        "SELECT logical, body FROM records"  # noqa: S608 - every value is quoted by literal()
        f" WHERE type = {literal(TASK_TYPE)} AND logical IS NOT NULL"
        f" AND (code IS NULL OR code IN {listed(TREE_CODES)})"
        f" AND json_extract(body, '$.status') IN {listed(UNFINISHED)}"
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
    waits = {path: waiting_on(board, path) for path, task in shown if task.after}
    done = finished(board, {place for targets in waits.values() for place in targets.values() if place is not None})
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


def waiting_on(board: Board, path: str) -> dict[str, Place | None]:
    """Each task the task's `after` reaches, once, by its display path, and where it is.

    A reference that reaches nothing is kept as written, with no place.
    """
    report = board.refs(path)
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
            # `refs` names the working tree `working-tree` and a commit `{"commit": id}`; `records` names them
            # `worktree` and the id.
            source = location["source"]
            place = ("worktree" if source == "working-tree" else source["commit"], location["path"])
            targets.setdefault(location["display"], place)
    return targets


def finished(board: Board, places: set[Place]) -> set[Place]:
    """Which of `places` hold `done` tasks.

    `records` gives a commit mounted at two paths only one display path, so a
    task is matched by its source and physical path, which every route shares.
    """
    if not places:
        return set()
    pairs = ", ".join(f"({literal(source)}, {literal(path)})" for source, path in sorted(places))
    rows = board.rows(
        "SELECT source, path FROM records"  # noqa: S608 - every value is quoted by literal()
        f" WHERE type = {literal(TASK_TYPE)} AND (source, path) IN (VALUES {pairs})"
        f" AND (code IS NULL OR code IN {listed(TREE_CODES)}) AND json_extract(body, '$.status') = 'done'",
    )
    return {(source, path) for source, path in rows}
