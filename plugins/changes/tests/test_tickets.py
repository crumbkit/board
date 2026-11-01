"""A change's tickets and pulls are typed references: `board check` reports a broken one, `board refs` finds it."""

import pytest
from conftest import Board, change, mount_repo, record

PATH = "/changes/t"


@pytest.fixture
def mounted(board: Board) -> Board:
    """The board with the repository a change names mounted, so only its tickets and pulls can break it."""
    mount_repo(board, {"README.md": "x\n"})
    return board


def code(board: Board) -> str | None:
    """The code `board check` gives the change, or None when it is valid."""
    found: str | None = board.json("check", PATH)["data"][0].get("code")
    return found


def test_check_reports_a_ticket_that_reaches_nothing_and_admits_one_that_does(mounted: Board) -> None:
    board = mounted
    board.write("changes/t.json", change(tickets=["/tickets/ACME-123"]))
    assert code(board) == "board::ref::dangling"
    board.write("tickets/ACME-123.json", record("jira/issue/v1"))
    assert code(board) is None


def test_check_reports_a_ticket_of_another_type_and_admits_each_issue_type(mounted: Board) -> None:
    board = mounted
    board.write("tickets/ACME-123.json", record("jira/issue/v1"))
    board.write("issues/octo-widgets-6.json", record("github/issue/v1"))
    board.write("pulls/octo-widgets-7.json", record("github/pull/v1"))
    board.write("changes/t.json", change(tickets=["/pulls/octo-widgets-7"]))
    assert code(board) == "board::ref::wrong_type"
    board.write("changes/t.json", change(tickets=["/tickets/ACME-123", "/issues/octo-widgets-6"]))
    assert code(board) is None


def test_check_reports_a_pull_of_another_type_and_admits_a_pull(mounted: Board) -> None:
    board = mounted
    board.write("issues/octo-widgets-6.json", record("github/issue/v1"))
    board.write("pulls/octo-widgets-7.json", record("github/pull/v1"))
    board.write("changes/t.json", change(pulls=["/issues/octo-widgets-6"]))
    assert code(board) == "board::ref::wrong_type"
    board.write("changes/t.json", change(pulls=["/pulls/octo-widgets-7"]))
    assert code(board) is None


def test_refs_finds_the_changes_that_reference_a_ticket_and_not_one_that_does_not(mounted: Board) -> None:
    board = mounted
    board.write("tickets/ACME-123.json", record("jira/issue/v1"))
    board.write("changes/t.json", change(tickets=["/tickets/ACME-123"]))
    board.write("changes/u.json", change())
    [answer] = board.json("refs", "/tickets/ACME-123")["data"]
    assert [(found["logical"], found["pointers"]) for found in answer["to"]] == [("/changes/t", ["/tickets/0"])]
