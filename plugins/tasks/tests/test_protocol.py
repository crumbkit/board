"""The plugin's side of the protocol, run directly with crafted requests."""

import json
import os
import subprocess
import sys
from pathlib import Path
from typing import Any

from conftest import Board, board_binary, task_v2


def plugin(request: dict[str, Any], cwd: Path) -> tuple[int, dict[str, Any]]:
    """Run the plugin once on `request`, with the tested `board` first on PATH: its exit code and document."""
    path = os.pathsep.join([str(board_binary().parent), os.environ.get("PATH", "")])
    ran = subprocess.run(
        [sys.executable, "-m", "board_tasks"],
        cwd=cwd,
        env={**os.environ, "PATH": path},
        input=json.dumps(request),
        capture_output=True,
        text=True,
        check=False,
    )
    document: dict[str, Any] = json.loads(ran.stdout)
    return ran.returncode, document


def call_request(root: Path, capability: str = "available") -> dict[str, Any]:
    return {
        "$type": "board/call/v1",
        "capability": capability,
        "input": {"repo": "/"},
        "board": {"root": str(root)},
    }


def test_a_board_that_cannot_run_or_refuses_is_reported_and_a_working_one_answers(board: Board, tmp_path: Path) -> None:
    code, document = plugin(call_request(tmp_path / "nowhere"), tmp_path)
    assert code == 1
    assert document["code"] == "tasks::board::failed"
    (tmp_path / "plain").mkdir()
    code, document = plugin(call_request(tmp_path / "plain"), tmp_path)
    assert (code, document["code"]) == (1, "tasks::board::refused")
    assert document["fields"] == {"code": "board::config::not_found"}
    board.write("tasks/a.md", task_v2("A", due="2026-10-01"))
    called = board.json("call", "tasks.available", "--repo", "/")
    assert called["schema"] == "board/call-report/v2"
    # A named repo is read without asking the caller's git remote.
    assert (called["data"]["output"]["repos"], called["data"]["output"]["origin_unread"]) == (["/"], None)
    unnamed = board.json("call", "tasks.available")["data"]["output"]
    assert unnamed["origin_unread"] is not None, unnamed


def test_a_capability_the_plugin_does_not_offer_is_refused(tmp_path: Path) -> None:
    code, document = plugin(call_request(tmp_path, capability="late"), tmp_path)
    assert (code, document["code"]) == (1, "tasks::call::unknown_capability")


def test_a_request_board_would_not_send_is_refused_and_its_call_twin_is_answered(board: Board, tmp_path: Path) -> None:
    request = call_request(board.root)
    request["$type"] = "board/extract/v1"
    code, document = plugin(request, tmp_path)
    assert (code, document["code"]) == (1, "tasks::protocol::bad_request")
    request["$type"] = "board/call/v1"
    assert plugin(request, tmp_path)[0] == 0
