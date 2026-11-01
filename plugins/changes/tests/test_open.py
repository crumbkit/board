"""`changes.open` and the change schema."""

import json

from conftest import Board, change, clone, mount, record, setup


def test_open_lists_changes_with_no_landed_or_dropped_record_by_path(board: Board) -> None:
    board.write("changes/b.json", change(route="unread"))
    board.write("changes/a.json", change())
    board.write("changes/c.json", change())
    board.write("changes/c/landed.json", record("changes/landed/v1", commit="a3f9c0d"))
    board.write("changes/d.json", change())
    board.write("changes/d/dropped.json", record("changes/dropped/v1", why="Not needed."))
    output = board.call("open", {})
    shown = {"title": "Let a suite declare a timeout", "repo": "/repos/app", "branch": "work"}
    unheld = {"holder": None, "worktree": None}
    assert output["changes"] == [
        {"path": "/changes/a", **shown, "route": "owner", **unheld},
        {"path": "/changes/b", **shown, "route": "unread", **unheld},
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


def claim(session: str, worktree: str = "/work/app") -> str:
    """A claim of a change by `session`, working in `worktree`."""
    return record("changes/claim/v1", session=session, worktree=worktree)


def test_each_change_names_its_holder_and_worktree_from_its_newest_claim(board: Board) -> None:
    board.write("changes/a.json", change())
    board.write("changes/a/claims/20261101T090000000000Z.json", claim("s1", "/work/one"))
    board.write("changes/a/claims/20261101T100000000000Z.json", claim("s2", "/work/two"))
    board.write("changes/b.json", change())
    output = board.call("open", {})
    held = {entry["path"]: (entry["holder"], entry["worktree"]) for entry in output["changes"]}
    assert held == {"/changes/a": ("s2", "/work/two"), "/changes/b": (None, None)}


def test_holder_keeps_only_the_changes_whose_newest_claim_names_the_session(board: Board) -> None:
    board.write("changes/a.json", change())
    board.write("changes/a/claims/20261101T090000000000Z.json", claim("s1"))
    board.write("changes/b.json", change())
    board.write("changes/b/claims/20261101T090000000000Z.json", claim("s1"))
    board.write("changes/b/claims/20261101T100000000000Z.json", claim("s2"))
    assert [entry["path"] for entry in board.call("open", {"holder": "s1"})["changes"]] == ["/changes/a"]
    assert [entry["path"] for entry in board.call("open", {"holder": "s2"})["changes"]] == ["/changes/b"]
    assert board.call("open", {"holder": "s3"})["total"] == 0


def test_remote_and_branch_keep_the_changes_to_that_repository_and_branch(board: Board) -> None:
    board.write("repos/app.json", record("board/repo/v1", remote="example/app"))
    board.write("repos/other.json", record("board/repo/v1", remote="example/other"))
    board.write("changes/a.json", change())
    board.write("changes/b.json", change(branch="next"))
    board.write("changes/c.json", change(repo="/repos/other"))
    on = {"remote": "https://github.com/example/app.git", "branch": "work"}
    assert [entry["path"] for entry in board.call("open", on)["changes"]] == ["/changes/a"]
    assert [entry["path"] for entry in board.call("open", {"remote": "example/app"})["changes"]] == [
        "/changes/a",
        "/changes/b",
    ]
    assert board.call("open", {"remote": "example/none", "branch": "work"})["total"] == 0


def test_remote_matches_a_mounted_repository_by_its_clones_origin(board: Board) -> None:
    cloned = clone(board)
    mount(board, cloned)
    board.write("changes/a.json", change())
    assert [entry["path"] for entry in board.call("open", {"remote": "example/app"})["changes"]] == ["/changes/a"]
    assert board.call("open", {"remote": "example/other"})["total"] == 0
