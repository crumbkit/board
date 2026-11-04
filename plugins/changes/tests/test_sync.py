"""`changes.sync`, `changes.drop` and the last sync that `open` and `next` report.

A stub gh, first on PATH, answers `gh api repos/<owner>/<repo>/pulls/<number>`
with `pulls/<number>.json` from its own directory, or with GitHub's 404 when
that file is missing, and with a 502 when `fail` is there. It records each
call in `gh-calls.txt`, so no test reaches GitHub.
"""

import dataclasses
import json
from pathlib import Path
from typing import Any

import pytest
from conftest import TODAY, Board, change, days_ago, record, task

GH = """#!/bin/sh
here="$(dirname "$0")"
echo "$@" >> "$here/gh-calls.txt"
if [ -f "$here/fail" ]; then echo "HTTP 502: Bad Gateway" >&2; exit 1; fi
number="${2##*/}"
if [ -f "$here/pulls/$number.json" ]; then cat "$here/pulls/$number.json"; exit 0; fi
echo "gh: Not Found (HTTP 404)" >&2
exit 1
"""
PULL = "https://github.com/octo/widgets/pull/{number}"
MERGE = "0123456789abcdef0123456789abcdef01234567"


@pytest.fixture
def gh(board: Board) -> tuple[Board, Path]:
    """The fixture board with the stub gh first on its PATH, and the stub's directory."""
    stub = board.root.parent / "gh-bin"
    (stub / "pulls").mkdir(parents=True)
    (stub / "gh").write_text(GH)
    (stub / "gh").chmod(0o755)
    return dataclasses.replace(board, bins=(stub,)), stub


def answers(stub: Path, number: int, **fields: Any) -> None:  # noqa: ANN401 - the pull request's fields
    """Make the stub gh answer pull request `number` with `fields`."""
    (stub / "pulls" / f"{number}.json").write_text(json.dumps({"number": number, **fields}))


def pulls(board: Board, stub: Path) -> None:
    """Changes whose pull requests merged, closed, stayed open and are unknown to GitHub, and three without one."""
    board.write("changes/merged.json", change("land", pull_request=PULL.format(number=1)))
    answers(stub, 1, state="closed", merged=True, merge_commit_sha=MERGE, closed_at="2026-11-02T10:00:00Z")
    board.write("changes/closed.json", change("build", pull_request=PULL.format(number=2)))
    answers(stub, 2, state="closed", merged=False, merge_commit_sha=None, closed_at="2026-11-03T09:00:00Z")
    board.write("changes/open.json", change("review", pull_request=PULL.format(number=3)))
    answers(stub, 3, state="open", merged=False)
    board.write("changes/gone.json", change("build", pull_request=PULL.format(number=4)))
    board.write("changes/elsewhere.json", change("build", pull_request="https://gitlab.com/o/r/-/merge_requests/1"))
    board.write("changes/bare.json", change("build"))
    board.write("changes/started.json", change("build", head="a3f9c0d"))
    board.write("changes/planning.json", change("plan", pull_request=PULL.format(number=5)))
    board.write("repos/other/changes/theirs.json", change("build", pull_request=PULL.format(number=6)))


def statuses(output: dict[str, Any]) -> dict[str, tuple[str, str | None, str | None]]:
    """Each entry's status, the phase it moved to and its code, by path."""
    return {entry["path"]: (entry["status"], entry["to"], entry["code"]) for entry in output["changes"]}


EXPECTED = {
    "/changes/bare": ("no_pull_request", None, "changes::sync::no_pull_request"),
    "/changes/closed": ("closed", "dropped", None),
    "/changes/elsewhere": ("not_github", None, "changes::sync::not_github"),
    "/changes/gone": ("not_found", None, "changes::sync::pull_not_found"),
    "/changes/merged": ("merged", "done", None),
    "/changes/open": ("open", None, None),
    "/changes/started": ("no_pull_request", None, "changes::sync::no_pull_request"),
}


def test_sync_ends_merged_and_closed_changes_and_reports_the_rest(gh: tuple[Board, Path]) -> None:
    board, stub = gh
    pulls(board, stub)
    output = board.call("sync", {})
    assert statuses(output) == EXPECTED
    assert (output["moved"], output["total"], output["dry_run"]) == (2, 7, False)
    merged = board.read("changes/merged.json")
    assert (merged["phase"], merged["merge_commit"]) == ("done", f"https://github.com/octo/widgets/commit/{MERGE}")
    closed = board.read("changes/closed.json")
    assert closed["phase"] == "dropped"
    assert (
        closed["dropped_reason"] == f"its pull request {PULL.format(number=2)} was closed without merging on 2026-11-03"
    )
    assert board.read("changes/open.json")["phase"] == "review"
    assert board.read("changes/sync.json") == {"$type": "changes/sync/v1", "synced": TODAY}
    commands = {entry["path"]: entry["command"] for entry in output["changes"]}
    assert commands["/changes/bare"] == "board call changes.drop --path /changes/bare --reason '<why>'"
    assert commands["/changes/started"] == "board call changes.next --path /changes/started"
    asked = (stub / "gh-calls.txt").read_text().splitlines()
    assert asked == [f"api repos/octo/widgets/pulls/{number}" for number in (2, 4, 1, 3)]


def test_a_dry_run_reports_the_same_and_writes_nothing(gh: tuple[Board, Path]) -> None:
    board, stub = gh
    pulls(board, stub)
    before = (board.root / "changes/merged.json").read_text()
    output = board.call("sync", {"dry_run": True})
    assert statuses(output) == EXPECTED
    assert output["dry_run"] is True
    assert (board.root / "changes/merged.json").read_text() == before
    assert not (board.root / "changes/sync.json").exists()


def test_a_merged_change_with_an_open_task_is_owed_and_moves_once_the_task_is_done(gh: tuple[Board, Path]) -> None:
    board, stub = gh
    board.write("tasks/left.md", task())
    board.write("changes/merged.json", change("build", pull_request=PULL.format(number=1), records=["/tasks/left.md"]))
    answers(stub, 1, state="closed", merged=True, merge_commit_sha=MERGE)
    owed = board.call("sync", {})["changes"][0]
    assert (owed["status"], owed["to"], owed["code"]) == ("owed", None, "changes::sync::owed")
    assert "/tasks/left.md" in owed["detail"]
    assert board.read("changes/merged.json")["phase"] == "build"
    board.write("tasks/left.md", task("done"))
    assert board.call("sync", {})["changes"][0]["to"] == "done"
    assert board.read("changes/merged.json")["phase"] == "done"


def test_a_gh_failure_refuses_the_whole_sync_and_writes_nothing(gh: tuple[Board, Path]) -> None:
    board, stub = gh
    pulls(board, stub)
    (stub / "fail").write_text("")
    refused = board.refusal("sync", {})
    assert refused["code"] == "changes::gh::failed"
    assert "HTTP 502" in refused["message"]
    assert board.read("changes/merged.json")["phase"] == "land"
    assert not (board.root / "changes/sync.json").exists()
    (stub / "fail").unlink()
    assert board.call("sync", {})["moved"] == len(["merged", "closed"])


def test_open_and_next_warn_that_the_sync_is_stale_until_it_ran_today(board: Board) -> None:
    board.write("changes/t.json", change("build"))
    for output in (board.call("open", {}), board.call("next", {"path": "/changes/t"})):
        assert output["sync"] == {
            "synced": None,
            "stale": True,
            "code": "changes::sync::stale",
            "command": "board call changes.sync",
        }
    board.write("changes/sync.json", record("changes/sync/v1", synced=days_ago(1)))
    assert board.call("open", {})["sync"]["code"] == "changes::sync::stale"
    board.write("changes/sync.json", record("changes/sync/v1", synced=TODAY))
    for output in (board.call("open", {}), board.call("next", {"path": "/changes/t"})):
        assert output["sync"] == {"synced": TODAY, "stale": False, "code": None, "command": "board call changes.sync"}


def test_drop_ends_an_open_change_with_its_reason_and_refuses_one_already_ended(board: Board) -> None:
    board.write("changes/t.json", change("build"))
    given = {"path": "/changes/t", "reason": "Superseded by another change."}
    assert board.call("drop", given) == {
        "path": "/changes/t",
        "from": "build",
        "to": "dropped",
        "task": "/tasks/given.md",
    }
    assert board.read("changes/t.json")["dropped_reason"] == "Superseded by another change."
    refused = board.refusal("drop", given)
    assert (refused["code"], refused["fields"]["phase"]) == ("changes::phase::final", "dropped")
