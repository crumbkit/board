"""The `import` capability, through the real `board`."""

import datetime
import json
from typing import Any

from conftest import Board, artifact

A = "https://claude.ai/artifact/A"
B = "https://claude.ai/artifact/B"
C = "https://claude.ai/code/artifact/0b8a7c6e-1f2d-4c3b-9a8e-7d6c5b4a3f21"
ACCOUNT = "owner@example.com"


def listing(*urls: str, **fields: Any) -> dict[str, Any]:  # noqa: ANN401 - the listing's other fields
    """An Artifact tool list result with a row for each of `urls`, as the tool returns it."""
    rows = [
        {"title": f"Page {url[-1]}", "url": url, "updatedAt": "2026-11-01T02:41:45Z", "rel": "mine"} for url in urls
    ]
    return {"artifacts": rows, "pins_enabled": True, **fields}


def imported(board: Board, given: dict[str, Any]) -> dict[str, Any]:
    """The output of `board call artifacts.import` given `given` on stdin."""
    report = board.json("call", "artifacts.import", stdin=json.dumps(given))
    assert report["schema"] == "board/call-report/v2", report
    output: dict[str, Any] = report["data"]["output"]
    return output


def refused(board: Board, given: dict[str, Any]) -> str:
    """The code `board call artifacts.import` refuses `given` with."""
    ran = board.run("--json", "call", "artifacts.import", stdin=json.dumps(given))
    assert ran.returncode != 0, ran.stdout
    code: str = json.loads(ran.stdout)["code"]
    return code


def stored(board: Board, path: str) -> dict[str, Any]:
    """The record at `path`, relative to the board's root."""
    record: dict[str, Any] = json.loads((board.root / path).read_text())
    return record


def test_import_writes_only_the_artifacts_the_board_does_not_have(board: Board) -> None:
    held = artifact(A, first_seen="2026-10-01T09:00:00Z")
    board.write("artifacts/A.json", held)
    output = imported(board, {"account": ACCOUNT, "listing": listing(A, B, C)})
    written = ["/artifacts/B", "/artifacts/0b8a7c6e-1f2d-4c3b-9a8e-7d6c5b4a3f21"]
    assert output == {"written": written, "known": 1, "truncated": None}
    assert (board.root / "artifacts/A.json").read_text() == held
    assert stored(board, "artifacts/B.json") == {
        "$type": "artifacts/artifact/v1",
        "url": B,
        "account": ACCOUNT,
        "first_seen": "2026-11-01T02:41:45Z",
    }
    assert not (board.root / "artifacts/B").exists()
    assert imported(board, {"account": ACCOUNT, "listing": listing(A, B, C)}) == {
        "written": [],
        "known": 3,
        "truncated": None,
    }


def test_a_row_without_its_update_time_is_first_seen_at_the_import(board: Board) -> None:
    given = listing(A)
    del given["artifacts"][0]["updatedAt"]
    before = datetime.datetime.now(datetime.UTC).replace(microsecond=0)
    imported(board, {"account": ACCOUNT, "listing": given})
    first_seen = datetime.datetime.fromisoformat(stored(board, "artifacts/A.json")["first_seen"])
    assert before <= first_seen <= datetime.datetime.now(datetime.UTC)


def test_a_truncated_listing_is_reported_with_its_total_and_a_whole_one_is_not(board: Board) -> None:
    output = imported(board, {"account": ACCOUNT, "listing": listing(A, B, truncated=True, total=238)})
    assert output["truncated"] == {
        "listed": 2,
        "total": 238,
        "message": "the listing holds the 2 most recently updated of 238 artifacts of owner@example.com, and the"
        " Artifact tool lists no more, so the rest are not imported; a regular sync catches each one when it is"
        " updated",
    }
    assert imported(board, {"account": ACCOUNT, "listing": listing(A, B, C, truncated=False)})["truncated"] is None
    assert imported(board, {"account": ACCOUNT, "listing": listing(A)})["truncated"] is None


def test_a_listing_that_is_not_the_artifact_tools_list_result_is_refused_and_one_that_is_is_admitted(
    board: Board,
) -> None:
    assert refused(board, {"account": ACCOUNT, "listing": {"items": listing(A)["artifacts"]}}) == (
        "board::plugin::bad_input"
    )
    untitled = listing(A)
    del untitled["artifacts"][0]["title"]
    assert refused(board, {"account": ACCOUNT, "listing": untitled}) == "board::plugin::bad_input"
    assert refused(board, {"account": ACCOUNT, "listing": listing("https://example.com/A")}) == (
        "board::plugin::bad_input"
    )
    assert imported(board, {"account": ACCOUNT, "listing": listing(A)})["written"] == ["/artifacts/A"]


def test_a_listing_of_shared_artifacts_is_refused_and_a_listing_of_the_accounts_own_is_admitted(board: Board) -> None:
    assert refused(board, {"account": ACCOUNT, "listing": listing(A, scope="shared")}) == "board::plugin::bad_input"
    assert refused(board, {"account": ACCOUNT, "listing": listing(A, scope="all")}) == "board::plugin::bad_input"
    assert imported(board, {"account": ACCOUNT, "listing": listing(A)})["written"] == ["/artifacts/A"]


def test_an_import_without_an_account_is_refused_and_one_with_it_is_admitted(board: Board) -> None:
    assert refused(board, {"account": "", "listing": listing(A)}) == "board::plugin::bad_input"
    assert refused(board, {"listing": listing(A)}) == "board::plugin::bad_input"
    assert imported(board, {"account": ACCOUNT, "listing": listing(A)})["written"] == ["/artifacts/A"]
