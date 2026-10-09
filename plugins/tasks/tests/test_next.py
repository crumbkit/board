"""The `next` capability, through the real `board`."""

import json
import shutil
import subprocess
from pathlib import Path
from typing import Any

from conftest import SCHEMA, Board, task


def call_next(board: Board, given: dict[str, Any] | None = None) -> dict[str, Any]:
    """The output of `board call tasks.next`."""
    report = board.json("call", "tasks.next", stdin=json.dumps(given or {}))
    assert report["$type"] == "board/call-report/v1", report
    output: dict[str, Any] = report["output"]
    return output


def listed(board: Board, given: dict[str, Any] | None = None) -> list[tuple[str, str, str | None]]:
    """The tasks `next` lists, as (path, status, due)."""
    return [(item["path"], item["status"], item["due"]) for item in call_next(board, given)["tasks"]]


def blocking(board: Board) -> dict[str, tuple[bool, list[str]]]:
    """Whether each listed task is blocked, and by what, by its path."""
    return {item["path"]: (item["blocked"], item["blocked_by"]) for item in call_next(board)["tasks"]}


def test_next_lists_open_and_waiting_tasks_by_due_date_with_undated_ones_last_then_by_path(board: Board) -> None:
    board.write("tasks/b.json", task("B", due="2026-10-05"))
    board.write("tasks/a.json", task("A", status="waiting", due="2026-10-05"))
    board.write("tasks/early.json", task("Early", due="2026-10-01"))
    board.write("tasks/z-undated.json", task("Z"))
    board.write("tasks/m-undated.json", task("M", status="waiting"))
    board.write("tasks/done.json", task("Done", status="done", due="2026-09-01"))
    board.write("tasks/dropped.json", task("Dropped", status="dropped", due="2026-09-01"))
    assert listed(board) == [
        ("/tasks/early", "open", "2026-10-01"),
        ("/tasks/a", "waiting", "2026-10-05"),
        ("/tasks/b", "open", "2026-10-05"),
        ("/tasks/m-undated", "waiting", None),
        ("/tasks/z-undated", "open", None),
    ]


def test_a_task_is_blocked_while_a_task_in_its_after_is_not_done(board: Board) -> None:
    board.write("tasks/open.json", task("Open"))
    board.write("tasks/finished.json", task("Finished", status="done"))
    board.write("tasks/abandoned.json", task("Abandoned", status="dropped"))
    board.write("tasks/on-open.json", task("On open", after=["/tasks/open", "/tasks/finished", "/tasks/open"]))
    board.write("tasks/on-finished.json", task("On finished", after=["/tasks/finished"]))
    board.write("tasks/on-dropped.json", task("On dropped", after=["/tasks/abandoned"]))
    board.write("tasks/on-nothing.json", task("On nothing", after=["/tasks/missing"]))
    assert blocking(board) == {
        "/tasks/on-dropped": (True, ["/tasks/abandoned"]),
        "/tasks/on-finished": (False, []),
        "/tasks/on-nothing": (True, ["/tasks/missing"]),
        "/tasks/on-open": (True, ["/tasks/open"]),
        "/tasks/open": (False, []),
    }
    board.write("tasks/open.json", task("Open", status="done"))
    assert blocking(board)["/tasks/on-open"] == (False, [])


def test_a_reference_through_a_link_reaches_the_task_it_names(board: Board) -> None:
    board.write("tasks/finished.json", task("Finished", status="done"))
    board.write("alias.json", '{"$type": "board/mount/v1", "path": "/tasks/finished"}')
    board.write("tasks/after.json", task("After", after=["/alias"]))
    assert blocking(board) == {"/tasks/after": (False, [])}


def test_a_done_task_in_a_commit_mounted_twice_unblocks_through_either_mount(board: Board, tmp_path: Path) -> None:
    other = tmp_path / "other"
    (other / ".board" / "types" / "tasks" / "task").mkdir(parents=True)
    (other / ".board" / "board.json").write_text('{"$type": "board/config/v1"}')
    (other / ".board" / "types" / "tasks" / "task" / "v1.schema.json").write_text(SCHEMA.read_text())
    (other / "tasks").mkdir()
    (other / "tasks" / "finished.json").write_text(task("Finished", status="done"))
    (other / "tasks" / "pending.json").write_text(task("Pending"))
    for command in (["init", "-q"], ["add", "."], ["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "t"]):
        subprocess.run(["git", *command], cwd=other, check=True)
    commit = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=other,
        capture_output=True,
        text=True,
        check=True,
    ).stdout.strip()
    mount = json.dumps({"$type": "board/mount/v1", "repo": str(other), "follow": "HEAD", "commit": commit})
    board.write("a.json", mount)
    board.write("z.json", mount)
    board.write("tasks/after.json", task("After", after=["/z/tasks/finished", "/z/tasks/pending"]))
    assert blocking(board)["/tasks/after"] == (True, ["/z/tasks/pending"])


def test_next_shows_at_most_its_limit_and_says_how_many_it_left_out(board: Board) -> None:
    for name in ("a", "b", "c"):
        board.write(f"tasks/{name}.json", task(name.upper()))
    cut = call_next(board, {"limit": 2})
    assert [item["path"] for item in cut["tasks"]] == ["/tasks/a", "/tasks/b"]
    assert (cut["total"], cut["truncated"]) == (3, True)
    whole = call_next(board, {"limit": 3})
    assert (len(whole["tasks"]), whole["total"], whole["truncated"]) == (3, 3, False)
    default = call_next(board)
    assert (len(default["tasks"]), default["total"], default["truncated"]) == (3, 3, False)


def test_a_limit_outside_one_to_a_hundred_is_refused_by_board_before_the_plugin_runs(board: Board) -> None:
    board.write("tasks/a.json", task("A"))
    for refused in (0, 101):
        error = board.json("call", "tasks.next", stdin=json.dumps({"limit": refused}))
        assert error["code"] == "board::plugin::bad_input", error
    for admitted in (1, 100):
        assert call_next(board, {"limit": admitted})["total"] == 1


def test_next_answers_the_same_from_a_cold_cache(board: Board) -> None:
    board.write("tasks/first.json", task("First", due="2026-10-01"))
    board.write("tasks/second.json", task("Second", after=["/tasks/first"]))
    warm = call_next(board)
    shutil.rmtree(board.cache)
    assert call_next(board) == warm
