"""`migrate`: a phase-based `changes/change/v1` becomes a `changes/change/v2` with its plan and opened records."""

import json
from pathlib import Path
from typing import Any

from conftest import Board, clone, git, mount, record

OLD_SCHEMA = (Path(__file__).parent / "fixtures" / "change-v1.schema.json").read_text()
URL = "https://github.com/example/app/pull/7"


def old(board: Board, name: str = "t", **fields: Any) -> None:  # noqa: ANN401 - the record's other fields
    """A v1 change at /changes/<name> to /repos/app in the build phase, with its plan's Markdown, and v1's schema."""
    board.write(".board/types/changes/change/v1.schema.json", OLD_SCHEMA)
    board.write(f"changes/{name}/plan.md", "# Plan\n")
    defaults: dict[str, Any] = {
        "title": "Let a suite declare a timeout",
        "mount": "/repos/app",
        "phase": "build",
        "plan": f"/changes/{name}/plan.md",
        "route": "unowned",
    }
    board.write(f"changes/{name}.json", record("changes/change/v1", **(defaults | fields)))


def mounted_app(board: Board) -> str:
    """A mounted /repos/app on branch `work`, whose CONTRIBUTING.md names /suites/checks; its head."""
    cloned = clone(board, {"CONTRIBUTING.md": "Run `/suites/checks`.\n"})
    mount(board, cloned)
    board.write("suites/checks.json", record("runs/suite/v1", subjects=["app"], method="Run it."))
    return git(cloned.path, "rev-parse", "HEAD")


def migrate(board: Board, **given: Any) -> dict[str, Any]:  # noqa: ANN401 - the input's fields
    """The output of `changes.migrate` with `given` on stdin."""
    return board.call("migrate", given, stdin=True)


def test_a_dry_run_lists_the_lines_and_writes_nothing(board: Board) -> None:
    head = mounted_app(board)
    old(board, head=head, pull_request=URL)
    output = migrate(board)
    assert (output["applied"], output["commit"], output["owed"]) == (False, None, [])
    paths = [line["path"] for line in output["lines"]]
    assert paths[0] == "/workflows/personal"
    assert paths[1] == "/changes/t"
    assert paths[2].startswith("/changes/t/plans/")
    assert paths[3:] == ["/changes/t/opened"]
    assert output["total"] == len(paths)
    assert board.read("changes/t.json")["$type"] == "changes/change/v1"
    assert not (board.root / "workflows").exists()


def test_apply_writes_the_v2_change_its_plan_and_opened_records_and_next_reads_them(board: Board) -> None:
    head = mounted_app(board)
    old(board, head=head, pull_request=URL, issue="https://github.com/example/app/issues/3")
    assert migrate(board, apply=True)["applied"] is True
    assert board.read("changes/t.json") == {
        "$type": "changes/change/v2",
        "title": "Let a suite declare a timeout",
        "repo": "/repos/app",
        "branch": "work",
        "workflow": "/workflows/personal",
        "suite": "/suites/checks",
        "route": "unowned",
        "issue": "https://github.com/example/app/issues/3",
    }
    [plan] = [name for name in board.listed("changes/t/plans") if name.endswith(".json")]
    assert json.loads((board.root / "changes/t/plans" / plan).read_text()) == {
        "$type": "changes/plan/v1",
        "text": "/changes/t/plan.md",
        "uses": [],
    }
    assert board.read("changes/t/opened.json") == {"$type": "changes/opened/v1", "pull_request": URL}
    assert board.call("next", {"path": "/changes/t"})["next"] == "build"
    assert migrate(board) == {"applied": False, "commit": None, "lines": [], "total": 0, "truncated": False, "owed": []}


def test_a_change_with_no_head_is_owed_and_one_given_its_branch_is_migrated(board: Board) -> None:
    mounted_app(board)
    old(board)
    output = migrate(board, apply=True)
    assert (output["total"], len(output["owed"])) == (0, 1)
    assert "/changes/t has no branch" in output["owed"][0]
    assert board.read("changes/t.json")["$type"] == "changes/change/v1"
    output = migrate(board, apply=True, changes={"/changes/t": {"branch": "work"}})
    assert output["owed"] == []
    assert board.read("changes/t.json")["branch"] == "work"


def test_an_unmounted_repository_owes_a_branch_and_suite_and_both_given_migrate_it(board: Board) -> None:
    board.write("repos/app.json", record("board/repo/v1", remote="example/app"))
    board.write("suites/checks.json", record("runs/suite/v1", subjects=["app"], method="Run it."))
    old(board, head="a" * 40)
    assert "/changes/t has no branch or suite" in migrate(board)["owed"][0]
    given = {"/changes/t": {"branch": "work", "suite": "/suites/checks"}}
    assert migrate(board, apply=True, changes=given)["owed"] == []
    assert board.read("changes/t.json")["suite"] == "/suites/checks"


def test_a_dropped_v1_change_gets_a_dropped_record_and_ends(board: Board) -> None:
    head = mounted_app(board)
    old(board, head=head, phase="dropped")
    migrate(board, apply=True)
    assert board.read("changes/t/dropped.json")["$type"] == "changes/dropped/v1"
    assert board.call("next", {"path": "/changes/t"})["ended"] == "dropped"


def test_a_done_v1_change_owes_its_merge_commit_and_one_given_it_lands(board: Board) -> None:
    head = mounted_app(board)
    old(board, head=head, phase="done")
    owed = migrate(board, apply=True)["owed"]
    assert len(owed) == 1
    assert "/changes/t has no commit" in owed[0]
    assert board.read("changes/t.json")["$type"] == "changes/change/v1"
    migrate(board, apply=True, changes={"/changes/t": {"commit": head}})
    assert board.read("changes/t/landed.json") == {"$type": "changes/landed/v1", "commit": head}
    assert board.call("next", {"path": "/changes/t"})["ended"] == "landed"


def test_a_branch_git_cannot_read_is_owed_and_a_given_one_is_admitted(board: Board) -> None:
    head = mounted_app(board)
    old(board, head=head)
    clone_dir = board.root.parent / "app-clone"
    (clone_dir / ".git").rename(clone_dir / "git-moved")
    assert "/changes/t has no branch" in migrate(board)["owed"][0]
    (clone_dir / "git-moved").rename(clone_dir / ".git")
    assert migrate(board)["owed"] == []
