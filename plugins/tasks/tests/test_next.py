"""The `next` capability, through the real `board`."""

import json
import shutil
import sqlite3
import subprocess
from contextlib import closing
from pathlib import Path
from typing import Any

from conftest import SCHEMA, Board, task


def call_next(board: Board, given: dict[str, Any] | None = None) -> dict[str, Any]:
    """The output of `board call tasks.next`."""
    report = board.json("call", "tasks.next", stdin=json.dumps(given or {}))
    assert report["schema"] == "board/call-report/v2", report
    output: dict[str, Any] = report["data"]["output"]
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


def test_a_done_task_check_refuses_still_blocks_and_one_degraded_only_by_the_tree_does_not(board: Board) -> None:
    board.write("tasks/finished.json", task("Finished", status="done", extra=1))
    board.write("tasks/after.json", task("After", after=["/tasks/finished"]))
    [reference] = board.json("refs", "/tasks/after", "--field", "/status")["data"][0]["from"]
    assert reference["target_code"] == "board::record::invalid"
    assert blocking(board)["/tasks/after"] == (True, ["/tasks/finished"])
    board.write("tasks/finished.json", task("Finished", status="done", notes="/nowhere"))
    [reference] = board.json("refs", "/tasks/after", "--field", "/status")["data"][0]["from"]
    assert reference["target_code"] == "board::ref::dangling"
    assert blocking(board)["/tasks/after"] == (False, [])


def test_a_done_record_of_another_type_still_blocks(board: Board) -> None:
    board.write(".board/types/x/note/v1.schema.json", '{"type": "object"}')
    board.write("notes/n.json", '{"$type": "x/note/v1", "status": "done"}')
    board.write("tasks/after.json", task("After", after=["/notes/n", "/tasks/finished"]))
    board.write("tasks/finished.json", task("Finished", status="done"))
    assert blocking(board)["/tasks/after"] == (True, ["/notes/n"])


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


def test_a_call_whose_own_extraction_fails_is_refused_with_its_code_and_one_whose_extraction_works_answers(
    board: Board,
) -> None:
    board.write("tasks/a.json", task("A"))
    assert call_next(board)["total"] == 1
    (database,) = board.cache.rglob("tasks@*.sqlite")
    with closing(sqlite3.connect(database)) as connection, connection:
        connection.execute("DROP TABLE tasks")
        connection.execute("CREATE TABLE tasks (blob TEXT PRIMARY KEY)")
    board.write("tasks/b.json", task("B"))
    error = board.json("call", "tasks.next", stdin="{}")
    assert (error["code"], error["fields"]["code"]) == ("tasks::board::refused", "board::plugin::failed"), error


def logged(board: Board) -> list[dict[str, Any]]:
    """The records of the timing log, nested runs included."""
    log = board.cache / "board" / "timings.jsonl"
    return [json.loads(line) for line in log.read_text().splitlines()] if log.exists() else []


def refs_runs(board: Board) -> int:
    """How many `board refs` runs the timing log holds, nested ones included."""
    return sum(1 for record in logged(board) if record["command"] == "refs")


def nested_runs(board: Board) -> list[str]:
    """The commands of the `board` runs that one `tasks.next` call starts, in order."""
    before = len(logged(board))
    call_next(board)
    return [record["command"] for record in logged(board)[before:] if record["depth"] > 0]


def test_next_runs_one_query_and_one_refs_when_a_shown_task_waits_and_only_the_query_when_none_does(
    board: Board,
) -> None:
    board.write("tasks/first.json", task("First"))
    board.write("tasks/finished.json", task("Finished", status="done"))
    assert nested_runs(board) == ["query"]
    board.write("tasks/after.json", task("After", after=["/tasks/first", "/tasks/finished", "/tasks/missing"]))
    assert nested_runs(board) == ["query", "refs"]
    assert blocking(board)["/tasks/after"] == (True, ["/tasks/first", "/tasks/missing"])


def test_next_runs_one_board_refs_however_many_shown_tasks_wait_and_none_when_none_wait(board: Board) -> None:
    board.write("tasks/first.json", task("First"))
    before = refs_runs(board)
    call_next(board)
    assert refs_runs(board) == before, "no task waits"
    for count in (1, 3, 5):
        board.write(f"tasks/after-{count}.json", task("After", after=["/tasks/first"]))
        for other in range(1, count):
            board.write(f"tasks/after-{count}-{other}.json", task("After", after=["/tasks/first"]))
        before = refs_runs(board)
        blocked = [item for item in call_next(board)["tasks"] if item["blocked"]]
        assert len(blocked) >= count
        assert refs_runs(board) == before + 1, f"{len(blocked)} waiting tasks"
