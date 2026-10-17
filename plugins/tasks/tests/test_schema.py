"""The `tasks/task/v1` schema the plugin ships: what `board check` admits and refuses."""

import json

from conftest import Board, task


def checked(board: Board) -> dict[str, str | None]:
    """Each task's code from `board check`, by its path; `None` when it is valid."""
    report = json.loads(board.run("--json", "check", "--all").stdout)
    return {entry["logical"]: entry.get("code") for entry in report["data"] if entry["logical"].startswith("/tasks/")}


def test_each_of_the_four_statuses_is_admitted_and_any_other_is_refused(board: Board) -> None:
    for status in ("open", "waiting", "done", "dropped"):
        board.write(f"tasks/{status}.json", task(status.title(), status=status))
    board.write("tasks/someday.json", task("Someday", status="someday"))
    assert checked(board) == {
        "/tasks/open": None,
        "/tasks/waiting": None,
        "/tasks/done": None,
        "/tasks/dropped": None,
        "/tasks/someday": "board::record::invalid",
    }


def test_a_task_needs_a_title_and_a_status(board: Board) -> None:
    board.write("tasks/whole.json", task("Whole"))
    board.write("tasks/untitled.json", '{"$type": "tasks/task/v1", "status": "open"}')
    board.write("tasks/statusless.json", '{"$type": "tasks/task/v1", "title": "Statusless"}')
    assert checked(board) == {
        "/tasks/whole": None,
        "/tasks/untitled": "board::record::invalid",
        "/tasks/statusless": "board::record::invalid",
    }


def test_a_due_date_must_be_a_date(board: Board) -> None:
    board.write("tasks/dated.json", task("Dated", due="2026-10-01"))
    board.write("tasks/misdated.json", task("Misdated", due="2026-13-01"))
    assert checked(board) == {"/tasks/dated": None, "/tasks/misdated": "board::record::invalid"}


def test_a_task_has_no_project_id_or_other_field(board: Board) -> None:
    board.write("tasks/plain.json", task("Plain"))
    board.write("tasks/project.json", task("Project", project="/projects/board"))
    board.write("tasks/id.json", task("Id", id="t-1"))
    assert checked(board) == {
        "/tasks/plain": None,
        "/tasks/project": "board::record::invalid",
        "/tasks/id": "board::record::invalid",
    }


def test_notes_reference_a_file_that_exists(board: Board) -> None:
    board.write("notes/plan.md", "# Plan\n")
    board.write("tasks/noted.json", task("Noted", notes="/notes/plan.md"))
    board.write("tasks/lost.json", task("Lost", notes="/notes/gone.md"))
    board.write("tasks/relative.json", task("Relative", notes="notes/plan.md"))
    assert checked(board) == {
        "/tasks/noted": None,
        "/tasks/lost": "board::ref::dangling",
        "/tasks/relative": "board::ref::malformed",
    }


def test_after_lists_references_to_tasks(board: Board) -> None:
    board.write("notes/plan.md", "# Plan\n")
    board.write("tasks/first.json", task("First"))
    board.write("tasks/second.json", task("Second", after=["/tasks/first"]))
    board.write("tasks/on-a-note.json", task("On a note", after=["/notes/plan.md"]))
    board.write("tasks/one-string.json", task("One string", after="/tasks/first"))
    assert checked(board) == {
        "/tasks/first": None,
        "/tasks/second": None,
        "/tasks/on-a-note": "board::ref::wrong_type",
        "/tasks/one-string": "board::record::invalid",
    }
