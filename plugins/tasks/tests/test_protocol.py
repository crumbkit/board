"""The plugin's side of the protocol, run directly with crafted requests."""

import json
import os
import sqlite3
import subprocess
import sys
from contextlib import closing
from pathlib import Path
from typing import Any

from conftest import Board, board_binary, task


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


def extract_request(database: Path, *records: tuple[str, str]) -> dict[str, Any]:
    return {
        "$type": "board/extract/v1",
        "records": [
            {"blob": blob, "path": f"/tasks/{blob}", "type": "tasks/task/v1", "record": json.loads(record)}
            for blob, record in records
        ],
        "board": {"root": str(database.parent), "database": str(database)},
    }


def rows(database: Path) -> list[tuple[str, str, str, str | None]]:
    with closing(sqlite3.connect(database)) as connection:
        return connection.execute("SELECT blob, title, status, due FROM tasks ORDER BY blob").fetchall()


def test_extraction_names_every_blob_and_replaces_each_blobs_row(tmp_path: Path) -> None:
    database = tmp_path / "tasks@0.1.0.sqlite"
    code, document = plugin(extract_request(database, ("a1", task("A")), ("b2", task("B", due="2026-10-01"))), tmp_path)
    assert (code, document) == (0, {"$type": "board/extract-result/v1", "blobs": ["a1", "b2"]})
    assert rows(database) == [("a1", "A", "open", None), ("b2", "B", "open", "2026-10-01")]
    code, _ = plugin(extract_request(database, ("a1", task("A again", status="done"))), tmp_path)
    assert code == 0
    assert rows(database) == [("a1", "A again", "done", None), ("b2", "B", "open", "2026-10-01")]


def call_request(root: Path, capability: str = "due") -> dict[str, Any]:
    return {
        "$type": "board/call/v1",
        "capability": capability,
        "input": {"before": "2026-10-02"},
        "board": {"root": str(root), "database": str(root / "unused.sqlite")},
    }


def test_a_board_that_cannot_run_or_refuses_is_reported_and_a_working_one_answers(board: Board, tmp_path: Path) -> None:
    code, document = plugin(call_request(tmp_path / "nowhere"), tmp_path)
    assert code == 1
    assert document["code"] == "tasks::board::failed"
    (tmp_path / "plain").mkdir()
    code, document = plugin(call_request(tmp_path / "plain"), tmp_path)
    assert (code, document["code"]) == (1, "tasks::board::refused")
    assert document["fields"] == {"code": "board::config::not_found"}
    board.write("tasks/a.json", task("A", due="2026-10-01"))
    called = board.json("call", "tasks.due", stdin='{"before": "2026-10-02"}')
    assert called["$type"] == "board/call-report/v1"


def test_a_capability_the_plugin_does_not_offer_is_refused(tmp_path: Path) -> None:
    code, document = plugin(call_request(tmp_path, capability="late"), tmp_path)
    assert (code, document["code"]) == (1, "tasks::call::unknown_capability")


def test_a_request_board_would_not_send_is_refused(tmp_path: Path) -> None:
    request = extract_request(tmp_path / "tasks.sqlite", ("a1", task("A")))
    request["colour"] = "blue"
    code, document = plugin(request, tmp_path)
    assert (code, document["code"]) == (1, "tasks::protocol::bad_request")
    del request["colour"]
    assert plugin(request, tmp_path)[0] == 0


def test_a_record_the_plugin_cannot_read_is_handled_without_a_row_and_logged(tmp_path: Path) -> None:
    database = tmp_path / "tasks@0.1.0.sqlite"
    misfit = '{"$type": "tasks/task/v1", "title": "Misfit", "status": "someday"}'
    ran = subprocess.run(
        [sys.executable, "-m", "board_tasks"],
        cwd=tmp_path,
        input=json.dumps(extract_request(database, ("a1", task("A")), ("m2", misfit))),
        capture_output=True,
        text=True,
        check=False,
    )
    assert (ran.returncode, json.loads(ran.stdout)["blobs"]) == (0, ["a1", "m2"])
    assert rows(database) == [("a1", "A", "open", None)]
    assert ran.stderr.count("\n") == 1
    assert "/tasks/m2" in ran.stderr
