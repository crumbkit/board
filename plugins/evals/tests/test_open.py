"""The `open` capability, through the real `board`."""

import shutil
from pathlib import Path

from conftest import Board, experiment, mount, repository


def test_open_lists_planned_and_running_evals_and_leaves_out_concluded_and_abandoned_ones(board: Board) -> None:
    board.write("evals/b-running.json", experiment("B?", ["serde"], runs=["/suites/s/runs/1"]))
    board.write("evals/a-planned.json", experiment("A?", ["gix"], informs="Pick a library."))
    board.write("evals/done.json", experiment("Done?", verdict="supported", conclusion="Yes."))
    board.write("evals/dropped.json", experiment("Dropped?", abandoned="Moot."))
    output = board.call("open", {})
    assert [(item["path"], item["status"]) for item in output["evals"]] == [
        ("/evals/a-planned", "planned"),
        ("/evals/b-running", "running"),
    ]
    assert output["evals"][0] == {
        "path": "/evals/a-planned",
        "question": "A?",
        "subjects": ["gix"],
        "status": "planned",
        "verdict": None,
        "informs": "Pick a library.",
        "decisions": [],
    }
    assert (output["total"], output["truncated"]) == (2, False)


def test_an_eval_that_is_removed_is_no_longer_open(board: Board) -> None:
    board.write("evals/a.json", experiment("A?"))
    board.write("evals/b.json", experiment("B?", runs=["/suites/s/runs/1"]))
    assert board.paths("open", {}) == ["/evals/a", "/evals/b"]
    (board.root / "evals" / "a.json").unlink()
    assert board.paths("open", {}) == ["/evals/b"]


def test_an_eval_that_concludes_leaves_the_open_list(board: Board) -> None:
    board.write("evals/a.json", experiment("A?", runs=["/suites/s/runs/1"]))
    assert board.paths("open", {}) == ["/evals/a"]
    board.write("evals/a.json", experiment("A?", verdict="refuted", conclusion="No."))
    assert board.paths("open", {}) == []


def test_open_answers_the_same_from_a_cold_cache(board: Board) -> None:
    board.write("evals/a.json", experiment("A?", runs=["/suites/s/runs/1"]))
    board.write("evals/b.json", experiment("B?"))
    board.write("evals/c.json", experiment("C?", abandoned="Moot."))
    warm = board.call("open", {})
    shutil.rmtree(board.cache)
    assert board.call("open", {}) == warm


def test_open_shows_at_most_its_limit_and_says_how_many_it_left_out(board: Board) -> None:
    for name in ("a", "b", "c"):
        board.write(f"evals/{name}.json", experiment(name.upper() + "?"))
    cut = board.call("open", {"limit": 2})
    assert [item["path"] for item in cut["evals"]] == ["/evals/a", "/evals/b"]
    assert (cut["total"], cut["truncated"]) == (3, True)
    for refused in (0, 101):
        assert board.json("call", "evals.open", "--limit", str(refused))["code"] == "board::plugin::bad_input"
    for admitted in (1, 100):
        assert board.call("open", {"limit": admitted})["total"] == len("abc")


def test_open_lists_the_open_evals_of_two_mounted_repositories(board: Board, tmp_path: Path) -> None:
    first, first_commit = repository(
        tmp_path,
        "first",
        {"evals/a.json": experiment("A?"), "evals/done.json": experiment("D?", abandoned="Moot.")},
    )
    second, second_commit = repository(
        tmp_path, "second", {"evals/b.json": experiment("B?", runs=["/suites/s/runs/1"])}
    )
    mount(board, "repos/first.json", first, first_commit)
    mount(board, "repos/second.json", second, second_commit)
    assert board.paths("open", {}) == ["/repos/first/evals/a", "/repos/second/evals/b"]
