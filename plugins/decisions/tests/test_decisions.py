"""The decision record as `board check` reads it, and the capabilities through the real `board`."""

import json

from conftest import SCHEMAS, Board, decision, question

D = "/projects/board/decisions/d"


def test_board_lists_the_plugin_as_enabled_and_enabling_wrote_the_schema_it_ships(board: Board) -> None:
    report = board.json("plugin", "list")
    plugin = next(plugin for plugin in report["data"] if plugin["name"] == "decisions")
    assert (plugin["status"], plugin["enabled"]) == ("ok", True)
    shipped = SCHEMAS / "decision" / "v1.schema.json"
    written = board.root / ".board" / "types" / "decisions" / "decision" / "v1.schema.json"
    assert json.loads(written.read_text()) == json.loads(shipped.read_text())


def test_check_reports_a_basis_or_supersedes_that_reaches_nothing_and_admits_ones_that_reach_a_record(
    board: Board,
) -> None:
    board.write("evals/e.json", {"$type": "evals/eval/v1", "question": "Q?", "subjects": ["gix"]})
    board.write("projects/board/decisions/old.json", decision())
    board.write(
        "projects/board/decisions/rests.json",
        decision(basis=["/evals/e"], supersedes=["/projects/board/decisions/old"]),
    )
    board.write("projects/board/decisions/basis-gone.json", decision(basis=["/evals/gone"]))
    board.write("projects/board/decisions/supersedes-gone.json", decision(supersedes=["/projects/board/decisions/x"]))
    codes = board.checked()
    assert {path: code for path, code in codes.items() if "/decisions/" in path} == {
        "/projects/board/decisions/old": None,
        "/projects/board/decisions/rests": None,
        "/projects/board/decisions/basis-gone": "board::ref::dangling",
        "/projects/board/decisions/supersedes-gone": "board::ref::dangling",
    }


def test_check_reports_a_supersedes_that_reaches_another_type_and_admits_one_that_reaches_a_decision(
    board: Board,
) -> None:
    board.write("projects/board/questions/q.json", question())
    board.write("projects/board/decisions/old.json", decision())
    board.write("projects/board/decisions/right.json", decision(supersedes=["/projects/board/decisions/old"]))
    board.write("projects/board/decisions/wrong.json", decision(supersedes=["/projects/board/questions/q"]))
    codes = board.checked()
    assert codes["/projects/board/decisions/right"] is None
    assert codes["/projects/board/decisions/wrong"] == "board::ref::wrong_type"


def test_open_lists_a_decision_an_open_question_asks_about_until_the_question_is_answered(board: Board) -> None:
    board.write("projects/board/decisions/d.json", decision(basis=["/evals/e"]))
    board.write("projects/board/decisions/other.json", decision())
    board.write("projects/board/questions/q.json", question(belongs_to=[D]))
    listed = board.call("decisions.open", {})
    assert [(item["path"], item["review"], item["questions"]) for item in listed["decisions"]] == [
        (D, "awaiting", ["/projects/board/questions/q"])
    ]
    board.write("projects/board/questions/q.json", question("answered", [D]))
    assert board.call("decisions.open", {})["total"] == 0
    about = board.call("decisions.about", {"ref": "/evals/e"})
    assert [(item["path"], item["review"]) for item in about["decisions"]] == [(D, "reviewed")]


def test_each_eval_lists_the_decisions_whose_basis_names_it(board: Board) -> None:
    board.write("evals/e.json", {"$type": "evals/eval/v1", "question": "Q?", "subjects": ["gix"]})
    board.write("evals/f.json", {"$type": "evals/eval/v1", "question": "F?", "subjects": ["gix"]})
    board.write("projects/board/decisions/d.json", decision(basis=["/evals/e"]))
    listed = board.call("evals.about", {"subject": "gix"})
    assert [(item["path"], item["decisions"]) for item in listed["evals"]] == [
        ("/evals/e", [D]),
        ("/evals/f", []),
    ]
