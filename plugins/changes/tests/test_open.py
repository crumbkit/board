"""`changes.open` and the change schema."""

import json

from conftest import Board, change, record, setup


def test_open_lists_changes_with_no_landed_or_dropped_record_by_path(board: Board) -> None:
    board.write("changes/b.json", change(route="unread"))
    board.write("changes/a.json", change())
    board.write("changes/c.json", change())
    board.write("changes/c/landed.json", record("changes/landed/v1", commit="a3f9c0d"))
    board.write("changes/d.json", change())
    board.write("changes/d/dropped.json", record("changes/dropped/v1", why="Not needed."))
    output = board.call("open", {})
    shown = {"title": "Let a suite declare a timeout", "repo": "/repos/app", "branch": "work"}
    assert output["changes"] == [
        {"path": "/changes/a", **shown, "route": "owner"},
        {"path": "/changes/b", **shown, "route": "unread"},
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


def test_the_schema_refuses_an_unknown_route_or_field_and_reports_a_reference_that_reaches_nothing(
    board: Board,
) -> None:
    setup(board)
    board.write("changes/route.json", change(route="later"))
    board.write("changes/extra.json", change(phase="plan"))
    board.write("changes/dangling.json", change(workflow="/workflows/none"))
    assert checked(board) == {
        "/changes/t": None,
        "/changes/route": "board::record::invalid",
        "/changes/extra": "board::record::invalid",
        "/changes/dangling": "board::ref::dangling",
    }


def test_the_schema_refuses_a_change_without_its_branch_and_admits_one_with_it(board: Board) -> None:
    setup(board)
    fields = json.loads(change())
    del fields["branch"]
    board.write("changes/bare.json", json.dumps(fields))
    assert checked(board) == {"/changes/t": None, "/changes/bare": "board::record::invalid"}
