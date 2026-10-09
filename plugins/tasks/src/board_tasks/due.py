"""The `due` capability: unfinished tasks due before a date, joined with Board's entries."""

from board_plugin.board import Board, literal
from board_plugin.protocol import CallRequest

from board_tasks.models import UNFINISHED, DueInput, DueOutput, DueTask


def due(request: CallRequest) -> DueOutput:
    """Ask Board, through `board query`, for the current tree's `open` and `waiting` tasks due before the date."""
    before = DueInput.model_validate(request.input).before
    # `board query` takes no parameters. The date is printed from a parsed
    # date, so it is always YYYY-MM-DD and cannot change the statement.
    unfinished = ", ".join(literal(status) for status in UNFINISHED)
    sql = (
        "SELECT min(e.logical) AS path, t.title, t.due"  # noqa: S608 - its values are that date and the quoted statuses
        " FROM p_tasks.tasks AS t JOIN entries AS e ON e.blob = t.blob"
        f" WHERE t.status IN ({unfinished}) AND t.due < '{before.isoformat()}'"
        " GROUP BY t.blob ORDER BY t.due, path"
    )
    rows, truncated = Board(request.board.root, "tasks").query(sql)
    tasks = [DueTask(path=path, title=title, due=day) for path, title, day in rows]
    return DueOutput(tasks=tasks, truncated=truncated)
