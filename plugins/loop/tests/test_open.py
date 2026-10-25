"""`loop.open` and the change schema."""

import json

from conftest import Board, change


def test_open_lists_changes_that_are_not_done_or_dropped_by_path(board: Board) -> None:
    board.write("changes/b.json", change("build", route="owner"))
    board.write("changes/a.json", change())
    board.write("changes/c.json", change("done"))
    board.write("changes/d.json", change("dropped"))
    output = board.call("open", {})
    assert output["changes"] == [
        {
            "path": "/changes/a",
            "title": "Let a suite declare a timeout",
            "repo": "/repos/x",
            "phase": "shape",
            "route": None,
        },
        {
            "path": "/changes/b",
            "title": "Let a suite declare a timeout",
            "repo": "/repos/x",
            "phase": "build",
            "route": "owner",
        },
    ]
    assert (output["total"], output["truncated"], output["unreadable"]) == (2, False, 0)


def test_open_returns_at_most_the_limit_and_says_how_many_it_left_out(board: Board) -> None:
    for name in ("a", "b", "c"):
        board.write(f"changes/{name}.json", change())
    output = board.call("open", {"limit": 2})
    assert [entry["path"] for entry in output["changes"]] == ["/changes/a", "/changes/b"]
    assert (output["total"], output["truncated"]) == (3, True)


def test_open_counts_a_degraded_change_as_unreadable_and_lists_a_valid_one(board: Board) -> None:
    board.write("changes/bad.json", change(route="sometimes"))
    board.write("changes/good.json", change())
    output = board.call("open", {})
    assert [entry["path"] for entry in output["changes"]] == ["/changes/good"]
    assert output["unreadable"] == 1


def checked(board: Board) -> dict[str, str | None]:
    """Each change's code from `board check`, by its path; `None` when it is valid."""
    report = json.loads(board.run("--json", "check", "--all").stdout)
    return {entry["logical"]: entry.get("code") for entry in report["data"] if entry["logical"].startswith("/changes/")}


def test_the_schema_refuses_an_unknown_phase_or_field_and_reports_a_reference_that_reaches_nothing(
    board: Board,
) -> None:
    board.write("repos/x/README.md", "x")
    board.write("tasks/a.md", "x")
    board.write("changes/good.json", change(records=["/tasks/a.md"], head="a3f9c0d"))
    board.write("changes/phase.json", change("later"))
    board.write("changes/extra.json", change(owner="someone"))
    board.write("changes/dangling.json", change(records=["/tasks/b"]))
    assert checked(board) == {
        "/changes/good": None,
        "/changes/phase": "board::record::invalid",
        "/changes/extra": "board::record::invalid",
        "/changes/dangling": "board::ref::dangling",
    }
