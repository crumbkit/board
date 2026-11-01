"""The `list` capability, through the real `board`."""

import shutil
from pathlib import Path

from conftest import Board, artifact, mount, repository, version

A = "https://claude.ai/artifact/A"
B = "https://claude.ai/artifact/B"
C = "https://claude.ai/artifact/C"


def test_list_gives_each_artifact_its_newest_version_newest_first(board: Board) -> None:
    board.write("artifacts/A.json", artifact(A))
    board.write("artifacts/A/versions/1.json", version("2026-10-01T09:00:00Z", version="1", title="Old"))
    board.write("artifacts/A/versions/1.html", "<p>old</p>")
    board.write("artifacts/A/versions/2.json", version("2026-10-03T09:00:00Z", version="2", title="New"))
    board.write("artifacts/B.json", artifact(B, first_seen="2026-10-02T09:00:00Z"))
    output = board.call("list", {})
    assert output == {
        "artifacts": [
            {
                "path": "/artifacts/A",
                "url": A,
                "account": "owner@example.com",
                "first_seen": "2026-10-31T18:20:00Z",
                "current": {
                    "path": "/artifacts/A/versions/2",
                    "version": "2",
                    "title": "New",
                    "at": "2026-10-03T09:00:00Z",
                },
            },
            {
                "path": "/artifacts/B",
                "url": B,
                "account": "owner@example.com",
                "first_seen": "2026-10-02T09:00:00Z",
                "current": None,
            },
        ],
        "total": 2,
        "truncated": False,
        "unreadable": 0,
    }


def test_a_version_keyed_by_its_hash_lists_no_version_id(board: Board) -> None:
    board.write("artifacts/A.json", artifact(A))
    board.write("artifacts/A/versions/0123456789abcdef.json", version(version=None))
    current = board.call("list", {})["artifacts"][0]["current"]
    assert (current["path"], current["version"]) == ("/artifacts/A/versions/0123456789abcdef", None)


def test_list_keeps_one_account_and_stops_at_the_limit(board: Board) -> None:
    board.write("artifacts/A.json", artifact(A, first_seen="2026-10-01T09:00:00Z"))
    board.write("artifacts/B.json", artifact(B, first_seen="2026-10-02T09:00:00Z", account="work@example.com"))
    board.write("artifacts/C.json", artifact(C, first_seen="2026-10-03T09:00:00Z"))
    mine = board.call("list", {"account": "owner@example.com", "limit": 1})
    assert [entry["path"] for entry in mine["artifacts"]] == ["/artifacts/C"]
    assert (mine["total"], mine["truncated"]) == (2, True)
    every = board.call("list", {"limit": 3})
    assert [entry["path"] for entry in every["artifacts"]] == ["/artifacts/C", "/artifacts/B", "/artifacts/A"]
    assert (every["total"], every["truncated"]) == (3, False)


def test_a_record_board_marks_degraded_is_counted_and_left_out_and_a_valid_one_is_listed(board: Board) -> None:
    board.write("artifacts/A.json", artifact(A))
    board.write("artifacts/A/versions/1.json", version(version="1"))
    board.write("artifacts/A/versions/2.json", version("2026-11-01T09:00:00Z", version="2", sha256="bad"))
    output = board.call("list", {})
    assert (output["artifacts"][0]["current"]["version"], output["unreadable"]) == ("1", 1)
    board.write("artifacts/A/versions/2.json", version("2026-11-01T09:00:00Z", version="2"))
    output = board.call("list", {})
    assert (output["artifacts"][0]["current"]["version"], output["unreadable"]) == ("2", 0)


def test_list_reads_artifacts_in_a_mounted_board(board: Board, tmp_path: Path) -> None:
    records = {"artifacts/A.json": artifact(A), "artifacts/A/versions/1.json": version(version="1")}
    repo, commit = repository(tmp_path, "other", records)
    mount(board, "boards/other.json", repo, commit)
    [entry] = board.call("list", {})["artifacts"]
    assert (entry["path"], entry["current"]["path"]) == (
        "/boards/other/artifacts/A",
        "/boards/other/artifacts/A/versions/1",
    )


def test_list_answers_the_same_from_a_cold_cache(board: Board) -> None:
    board.write("artifacts/A.json", artifact(A))
    board.write("artifacts/A/versions/1.json", version(version="1"))
    warm = board.call("list", {})
    shutil.rmtree(board.cache)
    assert board.call("list", {}) == warm


def test_for_keeps_the_artifacts_with_a_version_that_belongs_to_the_reference(board: Board) -> None:
    board.write("artifacts/A.json", artifact(A))
    board.write("artifacts/A/versions/1.json", version("2026-10-01T09:00:00Z", version="1", **{"for": ["/changes/t"]}))
    board.write("artifacts/A/versions/2.json", version("2026-10-02T09:00:00Z", version="2"))
    board.write("artifacts/B.json", artifact(B))
    board.write(
        "artifacts/B/versions/1.json",
        version("2026-10-31T18:20:00Z", version="1", **{"for": ["/changes/u", "/evals/e"]}),
    )
    board.write("artifacts/C.json", artifact(C))
    on_t = board.call("list", {"for": "/changes/t"})
    assert [(entry["path"], entry["current"]["version"]) for entry in on_t["artifacts"]] == [("/artifacts/A", "2")]
    assert on_t["total"] == 1
    assert [entry["path"] for entry in board.call("list", {"for": "/evals/e"})["artifacts"]] == ["/artifacts/B"]
    assert board.call("list", {"for": "/changes/none"})["total"] == 0
    assert {entry["path"] for entry in board.call("list", {})["artifacts"]} == {
        "/artifacts/A",
        "/artifacts/B",
        "/artifacts/C",
    }
