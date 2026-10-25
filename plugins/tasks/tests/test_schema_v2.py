"""The `tasks/task/v2` schema the plugin ships: what `board check` admits and refuses in a Markdown task."""

import json

from conftest import Board, task_v2


def checked(board: Board) -> dict[str, str | None]:
    """Each task's code from `board check`, by its path; `None` when it is valid."""
    report = json.loads(board.run("--json", "check", "--all").stdout)
    return {entry["logical"]: entry.get("code") for entry in report["data"] if entry["logical"].startswith("/tasks/")}


def test_each_of_the_three_statuses_is_admitted_and_waiting_is_refused(board: Board) -> None:
    for status in ("open", "done", "dropped", "waiting"):
        board.write(f"tasks/{status}.md", task_v2(status.title(), status=status))
    assert checked(board) == {
        "/tasks/open.md": None,
        "/tasks/done.md": None,
        "/tasks/dropped.md": None,
        "/tasks/waiting.md": "board::record::invalid",
    }


def test_defer_is_a_date_a_date_time_or_a_named_horizon(board: Board) -> None:
    board.write("tasks/date.md", task_v2("Date", defer="2026-10-26"))
    board.write("tasks/time.md", task_v2("Time", defer="2026-10-26T17:00:00+03:00"))
    board.write("tasks/horizon.md", task_v2("Horizon", defer="someday"))
    board.write("tasks/unnamed.md", task_v2("Unnamed", defer="next-year"))
    board.write("tasks/bare.md", "---\n$type: tasks/task/v2\ntitle: Bare\nstatus: open\ndefer: 2026-10-26\n---\n")
    assert checked(board) == {
        "/tasks/date.md": None,
        "/tasks/time.md": None,
        "/tasks/horizon.md": None,
        "/tasks/unnamed.md": "board::record::invalid",
        "/tasks/bare.md": None,
    }


def test_repeat_is_one_of_the_five_rule_forms(board: Board) -> None:
    admitted = ("daily", "weekdays", "weekly-mon", "monthly-31", "annually-02-29")
    for rule in admitted:
        board.write(f"tasks/{rule}.md", task_v2(rule, repeat=rule))
    for rule in ("weekly-monday", "monthly-32", "annually-13-01", "hourly"):
        board.write(f"tasks/{rule}.md", task_v2(rule, repeat=rule))
    assert checked(board) == {
        **{f"/tasks/{rule}.md": None for rule in admitted},
        "/tasks/weekly-monday.md": "board::record::invalid",
        "/tasks/monthly-32.md": "board::record::invalid",
        "/tasks/annually-13-01.md": "board::record::invalid",
        "/tasks/hourly.md": "board::record::invalid",
    }


def test_ephemeral_needs_repeat(board: Board) -> None:
    board.write("tasks/repeating.md", task_v2("Repeating", repeat="daily", ephemeral=True))
    board.write("tasks/once.md", task_v2("Once", ephemeral=True))
    assert checked(board) == {"/tasks/repeating.md": None, "/tasks/once.md": "board::record::invalid"}


def test_from_hour_is_an_hour_of_the_day_and_contexts_are_lowercase_tags(board: Board) -> None:
    board.write("tasks/evening.md", task_v2("Evening", from_hour=23, contexts=["phone", "errands"]))
    board.write("tasks/midnight.md", task_v2("Midnight", from_hour=24))
    board.write("tasks/shouting.md", task_v2("Shouting", contexts=["Phone"]))
    assert checked(board) == {
        "/tasks/evening.md": None,
        "/tasks/midnight.md": "board::record::invalid",
        "/tasks/shouting.md": "board::record::invalid",
    }


def test_after_references_v2_tasks_by_their_path_with_md(board: Board) -> None:
    board.write("tasks/first.md", task_v2("First"))
    board.write("tasks/old.json", '{"$type": "tasks/task/v1", "title": "Old", "status": "open"}')
    board.write("tasks/second.md", task_v2("Second", after=["/tasks/first.md"]))
    board.write("tasks/on-v1.md", task_v2("On v1", after=["/tasks/old"]))
    board.write("tasks/no-suffix.md", task_v2("No suffix", after=["/tasks/first"]))
    codes = checked(board)
    del codes["/tasks/old"]
    assert codes == {
        "/tasks/first.md": None,
        "/tasks/second.md": None,
        "/tasks/on-v1.md": "board::ref::wrong_type",
        "/tasks/no-suffix.md": "board::ref::dangling",
    }


def test_a_task_admits_fields_another_tool_writes_and_still_checks_its_own(board: Board) -> None:
    board.write("tasks/reflect.md", task_v2("Reflect", id="0f3c9a", pinned=True))
    board.write("tasks/reflect-bad.md", task_v2("Reflect bad", id="0f3c9b", pinned=True, status="waiting"))
    assert checked(board) == {"/tasks/reflect.md": None, "/tasks/reflect-bad.md": "board::record::invalid"}
