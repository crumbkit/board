"""The `due` capability and the tasks table, through the real `board`."""

import json

from conftest import Board, task


def due(board: Board, before: str) -> list[tuple[str, str, str]]:
    """The tasks `board call tasks.due` returns, as (path, title, due)."""
    report = board.json("call", "tasks.due", stdin=json.dumps({"before": before}))
    assert report["$type"] == "board/call-report/v1", report
    output = report["output"]
    assert output["truncated"] is False
    return [(item["path"], item["title"], item["due"]) for item in output["tasks"]]


def test_due_returns_open_tasks_due_before_the_date_by_due_date_then_path(board: Board) -> None:
    board.write("tasks/b.json", task("B", due="2026-10-05"))
    board.write("tasks/a.json", task("A", due="2026-10-05"))
    board.write("tasks/early.json", task("Early", due="2026-10-01"))
    board.write("tasks/done.json", task("Done", status="done", due="2026-10-01"))
    board.write("tasks/later.json", task("Later", due="2026-11-01"))
    board.write("tasks/someday.json", task("Someday"))
    assert due(board, "2026-10-06") == [
        ("/tasks/early", "Early", "2026-10-01"),
        ("/tasks/a", "A", "2026-10-05"),
        ("/tasks/b", "B", "2026-10-05"),
    ]
    assert due(board, "2026-10-05") == [("/tasks/early", "Early", "2026-10-01")]


def test_an_edited_task_replaces_its_row(board: Board) -> None:
    board.write("tasks/a.json", task("A", due="2026-10-01"))
    assert due(board, "2026-10-02") == [("/tasks/a", "A", "2026-10-01")]
    board.write("tasks/a.json", task("A", status="done", due="2026-10-01"))
    assert due(board, "2026-10-02") == []


def test_a_removed_task_is_not_due_though_its_row_remains(board: Board) -> None:
    board.write("tasks/a.json", task("A", due="2026-10-01"))
    assert due(board, "2026-10-02") == [("/tasks/a", "A", "2026-10-01")]
    (board.root / "tasks" / "a.json").unlink()
    assert due(board, "2026-10-02") == []


def test_board_query_joins_the_tasks_table_with_entries(board: Board) -> None:
    board.write("tasks/a.json", task("A", due="2026-10-01"))
    board.write("tasks/b.json", task("B", status="done"))
    report = board.json(
        "query",
        "SELECT e.logical, t.status, t.due FROM p_tasks.tasks AS t"
        " JOIN entries AS e ON e.blob = t.blob ORDER BY e.logical",
    )
    assert report["plugins"] == []
    assert report["rows"] == [["/tasks/a", "open", "2026-10-01"], ["/tasks/b", "done", None]]


def test_a_date_that_is_not_a_date_is_refused_by_board_before_the_plugin_runs(board: Board) -> None:
    board.write("tasks/a.json", task("A", due="2026-10-01"))
    refused = board.json("call", "tasks.due", stdin='{"before": "2026-13-01"}')
    assert refused["code"] == "board::plugin::bad_input"
    admitted = board.json("call", "tasks.due", stdin='{"before": "2026-12-01"}')
    assert admitted["$type"] == "board/call-report/v1"


def test_a_second_plugin_calls_due_by_capability(board: Board) -> None:
    board.write("tasks/a.json", task("A", due="2026-10-01"))
    report = board.json("call", "digest.overdue", stdin='{"before": "2026-10-02"}')
    assert report["$type"] == "board/call-report/v1", report
    assert report["output"] == {
        "overdue": {"tasks": [{"path": "/tasks/a", "title": "A", "due": "2026-10-01"}], "truncated": False},
    }
