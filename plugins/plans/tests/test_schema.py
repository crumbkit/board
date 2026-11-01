"""The schemas the plugin ships: what `board check` admits and refuses."""

import json

from conftest import Board, question

INVALID = "board::record::invalid"


def checked(board: Board, under: str) -> dict[str, str | None]:
    """Each record's code from `board check` under `under`, by its path; `None` when it is valid."""
    report = json.loads(board.run("--json", "check", "--all").stdout)
    return {entry["logical"]: entry.get("code") for entry in report["data"] if entry["logical"].startswith(under)}


def test_a_question_has_one_of_three_statuses_and_an_asking_session(board: Board) -> None:
    board.write("projects/a/questions/open.json", question())
    board.write("projects/a/questions/withdrawn.json", question("withdrawn"))
    board.write("projects/a/questions/closed.json", question("closed"))
    board.write("projects/a/questions/nameless.json", question(session="someone"))
    assert checked(board, "/projects/a/questions/") == {
        "/projects/a/questions/open": None,
        "/projects/a/questions/withdrawn": None,
        "/projects/a/questions/closed": INVALID,
        "/projects/a/questions/nameless": INVALID,
    }


def test_an_answer_references_a_question(board: Board) -> None:
    board.write("projects/a/questions/q.json", question("answered"))
    board.write("projects/a/questions/notes.md", "background")
    answer = {"$type": "plans/answer/v1", "choice": "none", "words": "Neither.", "answered": "2026-10-02T09:00:00Z"}
    board.write("projects/a/answers/q.json", json.dumps({**answer, "question": "/projects/a/questions/q"}))
    board.write("projects/a/answers/n.json", json.dumps({**answer, "question": "/projects/a/questions/notes.md"}))
    assert checked(board, "/projects/a/answers/") == {
        "/projects/a/answers/q": None,
        "/projects/a/answers/n": "board::ref::wrong_type",
    }


def test_an_answer_a_hook_wrote_names_the_plugin_and_hook(board: Board) -> None:
    board.write("projects/a/questions/q.json", question("answered"))
    answer = {
        "$type": "plans/answer/v1",
        "question": "/projects/a/questions/q",
        "choice": "a",
        "words": "Merge",
        "answered": "2026-10-02T09:00:00Z",
    }
    board.write("projects/a/answers/hooked.json", json.dumps({**answer, "written_by": "plans/pane-answer"}))
    board.write("projects/a/answers/prose.json", json.dumps({**answer, "written_by": "the pane"}))
    assert checked(board, "/projects/a/answers/") == {
        "/projects/a/answers/hooked": None,
        "/projects/a/answers/prose": INVALID,
    }
