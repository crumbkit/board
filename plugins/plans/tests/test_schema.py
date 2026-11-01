"""The schemas the plugin ships: what `board check` admits and refuses."""

import json

from conftest import Board, decision, question

INVALID = "board::record::invalid"


def checked(board: Board, under: str) -> dict[str, str | None]:
    """Each record's code from `board check` under `under`, by its path; `None` when it is valid."""
    report = json.loads(board.run("--json", "check", "--all").stdout)
    return {entry["logical"]: entry.get("code") for entry in report["data"] if entry["logical"].startswith(under)}


def test_a_decision_needs_a_reason_unless_a_hook_wrote_it_as_a_stub(board: Board) -> None:
    board.write("decisions/reasoned.json", decision())
    board.write("decisions/unreasoned.json", decision(why=""))
    board.write("decisions/stub.json", decision(why="", written_by="runs/pr-create", review="wanted"))
    assert checked(board, "/decisions/") == {
        "/decisions/reasoned": None,
        "/decisions/unreasoned": INVALID,
        "/decisions/stub": None,
    }


def test_a_stub_still_carries_why_even_when_it_is_empty(board: Board) -> None:
    stub = json.loads(decision(why="", written_by="runs/pr-create"))
    board.write("decisions/empty.json", json.dumps(stub))
    del stub["why"]
    board.write("decisions/missing.json", json.dumps(stub))
    assert checked(board, "/decisions/") == {"/decisions/empty": None, "/decisions/missing": INVALID}


def test_written_by_names_a_plugin_and_a_hook_and_nothing_else(board: Board) -> None:
    board.write("decisions/hooked.json", decision(written_by="runs/pr-create"))
    board.write("decisions/prose.json", decision(written_by="the runs hook"))
    board.write("decisions/empty.json", decision(written_by=""))
    assert checked(board, "/decisions/") == {
        "/decisions/hooked": None,
        "/decisions/prose": INVALID,
        "/decisions/empty": INVALID,
    }


def test_a_decision_keeps_the_fields_it_had_and_refuses_others(board: Board) -> None:
    full = decision(alternatives=["Wait."], urls=["https://example.com/pull/1"], review="none")
    board.write("decisions/full.json", full)
    board.write("decisions/extra.json", decision(owner="someone"))
    assert checked(board, "/decisions/") == {"/decisions/full": None, "/decisions/extra": INVALID}


def test_a_decision_refers_to_board_entries_in_refs_and_to_web_pages_in_urls(board: Board) -> None:
    board.write("projects/a/questions/q.json", question("answered"))
    board.write("decisions/ref.json", decision(refs=["/projects/a/questions/q"]))
    board.write("decisions/ref-url.json", decision(refs=["https://example.com/pull/1"]))
    board.write("decisions/url.json", decision(urls=["https://example.com/pull/1"]))
    board.write("decisions/url-path.json", decision(urls=["/projects/a/questions/q"]))
    board.write("decisions/url-hostless.json", decision(urls=["https://"]))
    assert checked(board, "/decisions/") == {
        "/decisions/ref": None,
        "/decisions/ref-url": "board::ref::malformed",
        "/decisions/url": None,
        "/decisions/url-path": INVALID,
        "/decisions/url-hostless": INVALID,
    }


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
