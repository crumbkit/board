"""What blocks a task in `available`, and how many `board` runs a call starts, through the real `board`.

Every call reads the board's own tree with `--repo /`, so it syncs no mount and reads no git remote, except
where a test mounts a notes repo of its own.
"""

import json
import sqlite3
from contextlib import closing
from typing import Any

from conftest import Board, git, notes_repo, task_v2


def available(board: Board, repo: str = "/") -> dict[str, Any]:
    """The output of `board call tasks.available --repo <repo>`."""
    report = board.json("call", "tasks.available", "--repo", repo)
    assert report["schema"] == "board/call-report/v2", report
    output: dict[str, Any] = report["data"]["output"]
    return output


def blocking(board: Board, repo: str = "/") -> dict[str, list[str]]:
    """What blocks each listed task, by its path. A task without `reviewed` is listed whether or not it waits."""
    output = available(board, repo)
    return {item["path"]: item["blocked_by"] for section in ("available", "stale") for item in output[section]["tasks"]}


def test_a_task_is_blocked_while_a_task_in_its_after_is_not_done(board: Board) -> None:
    board.write("tasks/open.md", task_v2("Open"))
    board.write("tasks/finished.md", task_v2("Finished", status="done"))
    board.write("tasks/abandoned.md", task_v2("Abandoned", status="dropped"))
    on_open = ["/tasks/open.md", "/tasks/finished.md", "/tasks/open.md"]
    board.write("tasks/on-open.md", task_v2("On open", after=on_open))
    board.write("tasks/on-finished.md", task_v2("On finished", after=["/tasks/finished.md"]))
    board.write("tasks/on-dropped.md", task_v2("On dropped", after=["/tasks/abandoned.md"]))
    board.write("tasks/on-nothing.md", task_v2("On nothing", after=["/tasks/missing.md"]))
    assert blocking(board) == {
        "/tasks/on-dropped.md": ["/tasks/abandoned.md"],
        "/tasks/on-finished.md": [],
        "/tasks/on-nothing.md": ["/tasks/missing.md"],
        "/tasks/on-open.md": ["/tasks/open.md"],
        "/tasks/open.md": [],
    }
    board.write("tasks/open.md", task_v2("Open", status="done"))
    assert blocking(board)["/tasks/on-open.md"] == []


def test_a_done_record_of_another_type_still_blocks_and_a_done_task_does_not(board: Board) -> None:
    board.write(".board/types/x/note/v1.schema.json", '{"type": "object"}')
    board.write("tasks/note.md", '---\n$type: x/note/v1\nstatus: "done"\n---\n')
    board.write("tasks/finished.md", task_v2("Finished", status="done"))
    board.write("tasks/after.md", task_v2("After", after=["/tasks/note.md", "/tasks/finished.md"]))
    assert blocking(board)["/tasks/after.md"] == ["/tasks/note.md"]


def test_a_done_task_check_refuses_still_blocks_and_one_degraded_only_by_the_tree_does_not(board: Board) -> None:
    board.write("tasks/finished.md", task_v2("Finished", status="done", repeat="hourly"))
    board.write("tasks/after.md", task_v2("After", after=["/tasks/finished.md"]))
    [reference] = board.json("refs", "/tasks/after.md", "--field", "/status")["data"][0]["from"]
    assert reference["target_code"] == "board::record::invalid"
    assert blocking(board)["/tasks/after.md"] == ["/tasks/finished.md"]
    board.write("tasks/finished.md", task_v2("Finished", status="done", after=["/tasks/nowhere.md"]))
    [reference] = board.json("refs", "/tasks/after.md", "--field", "/status")["data"][0]["from"]
    assert reference["target_code"] == "board::ref::dangling"
    assert blocking(board)["/tasks/after.md"] == []


def test_a_reference_through_a_link_reaches_the_task_it_names(board: Board) -> None:
    board.write("tasks/finished.md", task_v2("Finished", status="done"))
    board.write("alias.json", '{"$type": "board/mount/v1", "path": "/tasks/finished.md"}')
    board.write("tasks/after.md", task_v2("After", after=["/alias"]))
    assert blocking(board) == {"/tasks/after.md": []}
    board.write("tasks/finished.md", task_v2("Finished"))
    assert blocking(board)["/tasks/after.md"] == ["/tasks/finished.md"]


def test_a_commit_mounted_twice_is_one_repo_whose_done_task_unblocks(board: Board) -> None:
    tasks = {
        "finished": task_v2("Finished", status="done"),
        "pending": task_v2("Pending"),
        "after": task_v2("After", after=["/tasks/finished.md", "/tasks/pending.md"]),
    }
    clone = notes_repo(board, "a", {"covers": []}, tasks)
    pointer = {
        "$type": "board/mount/v1",
        "repo": str(clone),
        "follow": "main",
        "commit": git(clone, "rev-parse", "HEAD"),
    }
    board.write("repos/z.json", json.dumps(pointer))
    assert blocking(board, "/repos/a")["/repos/a/tasks/after.md"] == ["/repos/a/tasks/pending.md"]
    refused = board.json("call", "tasks.available", "--repo", "/repos/z")
    assert (refused["code"], refused["fields"]["repos"]) == ("tasks::repo::not_found", ["/", "/repos/a"])


def test_a_call_whose_own_extraction_fails_is_refused_with_its_code_and_one_whose_extraction_works_answers(
    board: Board,
) -> None:
    board.write("tasks/a.md", task_v2("A"))
    assert available(board)["available"]["total"] == 1
    (database,) = board.cache.rglob("tasks@*.sqlite")
    with closing(sqlite3.connect(database)) as connection, connection:
        # The table keeps the columns `available` reads, so its query prepares, and extraction's insert fails.
        connection.execute("CREATE TRIGGER refuse BEFORE INSERT ON tasks_v2 BEGIN SELECT RAISE(ABORT, 'no'); END")
    board.write("tasks/b.md", task_v2("B"))
    error = board.json("call", "tasks.available", "--repo", "/")
    assert (error["code"], error["fields"]["code"]) == ("tasks::board::refused", "board::plugin::failed"), error


def logged(board: Board) -> list[dict[str, Any]]:
    """The records of the timing log, nested runs included."""
    log = board.cache / "board" / "timings.jsonl"
    return [json.loads(line) for line in log.read_text().splitlines()] if log.exists() else []


def nested_runs(board: Board) -> list[str]:
    """The commands of the `board` runs that one `tasks.available --repo /` call starts, in order."""
    before = len(logged(board))
    available(board)
    return [record["command"] for record in logged(board)[before:] if record["depth"] > 0]


def test_a_call_runs_one_query_and_one_refs_when_a_task_waits_and_only_the_query_when_none_does(
    board: Board,
) -> None:
    board.write("tasks/first.md", task_v2("First"))
    board.write("tasks/finished.md", task_v2("Finished", status="done"))
    assert nested_runs(board) == ["query"]
    after = ["/tasks/first.md", "/tasks/finished.md", "/tasks/missing.md"]
    board.write("tasks/after.md", task_v2("After", after=after))
    assert nested_runs(board) == ["query", "refs"]
    assert blocking(board)["/tasks/after.md"] == ["/tasks/first.md", "/tasks/missing.md"]


def test_a_call_runs_one_board_refs_however_many_tasks_wait(board: Board) -> None:
    board.write("tasks/first.md", task_v2("First"))
    for count in (1, 3, 5):
        for waiter in range(count):
            board.write(f"tasks/after-{count}-{waiter}.md", task_v2("After", after=["/tasks/first.md"]))
        assert nested_runs(board) == ["query", "refs"], f"{count} more waiting tasks"
        assert sum(1 for blockers in blocking(board).values() if blockers) == sum(range(1, count + 1, 2))
