"""The `due` capability: open tasks due before a date, joined with Board's entries."""

import json
import subprocess

from board_tasks.models import DueInput, DueOutput, DueTask
from board_tasks.protocol import CallRequest, PluginError

# The most rows the one query returns; more are reported as truncated.
LIMIT = 10_000


def due(request: CallRequest) -> DueOutput:
    """Ask Board, through `board query`, for the current tree's open tasks due before the date."""
    before = DueInput.model_validate(request.input).before
    # `board query` takes no parameters. The date is printed from a parsed
    # date, so it is always YYYY-MM-DD and cannot change the statement.
    sql = (
        "SELECT min(e.logical) AS path, t.title, t.due"  # noqa: S608 - its one value is that date
        " FROM p_tasks.tasks AS t JOIN entries AS e ON e.blob = t.blob"
        f" WHERE t.status = 'open' AND t.due < '{before.isoformat()}'"
        " GROUP BY t.blob ORDER BY t.due, path"
    )
    try:
        ran = subprocess.run(  # noqa: S603 - the program is Board itself, found on PATH
            ["board", "query", "--json", "--limit", str(LIMIT), sql],  # noqa: S607
            cwd=request.board.root,
            capture_output=True,
            check=False,
        )
    except OSError as error:
        raise PluginError("tasks::query::failed", f"board query cannot be run: {error}") from error
    report = json.loads(ran.stdout or b"{}")
    if ran.returncode != 0:
        raise PluginError(
            "tasks::query::failed",
            "board query failed while finding due tasks",
            {"code": report.get("code"), "stderr": ran.stderr.decode(errors="replace")},
        )
    tasks = [DueTask(path=path, title=title, due=day) for path, title, day in report["rows"]]
    return DueOutput(tasks=tasks, truncated=report["truncated"])
