"""How the library reads what `board` prints, against a fake `board` and the real one."""

import json
import os
import subprocess
from pathlib import Path
from typing import Any

import pytest

from board_plugin import board
from board_plugin.board import Board
from board_plugin.protocol import PluginError

REPOSITORY = Path(__file__).resolve().parents[3]
DEMO = Board("/b", "demo")


def printing(monkeypatch: pytest.MonkeyPatch, stdout: bytes, code: int = 0, stderr: bytes = b"logs") -> None:
    """Make every `board` the library runs print `stdout` and `stderr` and exit with `code`."""

    def run(_self: Board, *arguments: str) -> subprocess.CompletedProcess[bytes]:
        return subprocess.CompletedProcess(["board", *arguments], code, stdout, stderr)

    monkeypatch.setattr(Board, "_run", run)


def report(rows: list[list[Any]], *, truncated: bool) -> bytes:
    """A `board/query-report/v1` holding `rows`."""
    return json.dumps({"$type": "board/query-report/v1", "rows": rows, "truncated": truncated}).encode()


def test_a_board_that_prints_no_json_is_a_failure_and_one_that_does_is_read(monkeypatch: pytest.MonkeyPatch) -> None:
    printing(monkeypatch, b"Usage: board [OPTIONS]")
    with pytest.raises(PluginError) as refused:
        DEMO.rows("SELECT 1")
    assert refused.value.code == "demo::board::failed"
    assert "first on PATH" in refused.value.message
    printing(monkeypatch, report([[1]], truncated=False))
    assert DEMO.rows("SELECT 1") == [[1]]


def test_a_board_error_is_a_refusal_carrying_boards_code_and_hint(monkeypatch: pytest.MonkeyPatch) -> None:
    error = {"$type": "board/error/v1", "code": "board::query::invalid", "message": "no such table", "hint": "fix it"}
    printing(monkeypatch, json.dumps(error).encode(), code=1)
    with pytest.raises(PluginError) as refused:
        DEMO.rows("SELECT 1")
    assert refused.value.code == "demo::board::refused"
    assert refused.value.fields == {"code": "board::query::invalid"}
    assert refused.value.message == "`board query` refused with board::query::invalid: no such table; fix it"


def test_a_failed_board_without_an_error_document_is_a_failure(monkeypatch: pytest.MonkeyPatch) -> None:
    printing(monkeypatch, json.dumps({"$type": "board/query-report/v1"}).encode(), code=1)
    with pytest.raises(PluginError) as refused:
        DEMO.rows("SELECT 1")
    assert refused.value.code == "demo::board::failed"


def test_a_truncated_query_is_refused_by_rows_and_reported_by_query(monkeypatch: pytest.MonkeyPatch) -> None:
    printing(monkeypatch, report([[1]], truncated=True))
    with pytest.raises(PluginError) as refused:
        DEMO.rows("SELECT 1")
    assert refused.value.code == "demo::board::truncated"
    assert DEMO.query("SELECT 1") == ([[1]], True)
    printing(monkeypatch, report([[1]], truncated=False))
    assert DEMO.rows("SELECT 1") == [[1]]


def test_a_literal_doubles_its_quotes() -> None:
    assert board.literal("it's") == "'it''s'"


@pytest.fixture
def real(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Board:
    """A fresh board, read through the workspace's debug `board`."""
    binary = Path(os.environ.get("BOARD_BIN", REPOSITORY / "target" / "debug" / "board")).resolve()
    monkeypatch.setenv("PATH", os.pathsep.join([str(binary.parent), os.environ.get("PATH", "")]))
    monkeypatch.setenv("XDG_CACHE_HOME", str(tmp_path / "cache"))
    root = tmp_path / "board"
    root.mkdir()
    subprocess.run(["git", "init", "-q"], cwd=root, check=True)
    subprocess.run([binary, "init"], cwd=root, check=True, capture_output=True)
    (root / "docs").mkdir()
    (root / "docs" / "a.md").write_text("one two")
    return Board(str(root), "demo")


def test_the_real_board_gives_a_files_bytes_and_refuses_a_directory_with_its_code(real: Board) -> None:
    assert real.get("/docs/a.md") == b"one two"
    with pytest.raises(PluginError) as refused:
        real.get("/docs")
    assert refused.value.code == "demo::board::refused"
    assert refused.value.fields["code"] == "board::record::not_a_record"


def test_the_real_board_resolves_a_path_and_a_missing_one_is_none(real: Board) -> None:
    found = real.refs("/docs/a.md")
    assert found is not None
    assert found["location"]["display"] == "/docs/a.md"
    assert real.refs("/docs/gone.md") is None
