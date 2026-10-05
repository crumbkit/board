"""How the plugin reads what `board` prints, and what it does when `board` misbehaves."""

import json
import subprocess
from typing import Any

import pytest

from board_controls import board
from board_controls.protocol import PluginError


def printing(monkeypatch: pytest.MonkeyPatch, stdout: bytes, code: int = 0) -> None:
    """Make every `board` the plugin runs print `stdout` and exit with `code`."""

    def run(_root: str, *arguments: str) -> subprocess.CompletedProcess[bytes]:
        return subprocess.CompletedProcess(["board", *arguments], code, stdout, b"logs")

    monkeypatch.setattr(board, "_run", run)


def report(rows: list[list[Any]], *, truncated: bool) -> bytes:
    """A `board/query-report/v1` holding `rows`."""
    return json.dumps({"$type": "board/query-report/v1", "rows": rows, "truncated": truncated}).encode()


def test_a_board_that_prints_no_json_is_a_failure_and_one_that_does_is_read(monkeypatch: pytest.MonkeyPatch) -> None:
    printing(monkeypatch, b"Usage: board [OPTIONS]")
    with pytest.raises(PluginError) as refused:
        board.query("/b", "SELECT 1")
    assert refused.value.code == "controls::board::failed"
    assert "first on PATH" in refused.value.message
    printing(monkeypatch, report([[1]], truncated=False))
    assert board.query("/b", "SELECT 1") == [[1]]


def test_a_board_error_is_a_refusal_carrying_boards_code_and_hint(monkeypatch: pytest.MonkeyPatch) -> None:
    error = {"$type": "board/error/v1", "code": "board::query::invalid", "message": "no such table", "hint": "fix it"}
    printing(monkeypatch, json.dumps(error).encode(), code=1)
    with pytest.raises(PluginError) as refused:
        board.query("/b", "SELECT 1")
    assert refused.value.code == "controls::board::refused"
    assert refused.value.fields == {"code": "board::query::invalid"}
    assert refused.value.message == "`board query` refused with board::query::invalid: no such table; fix it"


def test_a_failed_board_without_an_error_document_is_a_failure(monkeypatch: pytest.MonkeyPatch) -> None:
    printing(monkeypatch, json.dumps({"$type": "board/query-report/v1"}).encode(), code=1)
    with pytest.raises(PluginError) as refused:
        board.query("/b", "SELECT 1")
    assert refused.value.code == "controls::board::failed"
    assert "first on PATH" in refused.value.message


def test_a_truncated_query_is_refused_and_a_whole_one_is_read(monkeypatch: pytest.MonkeyPatch) -> None:
    printing(monkeypatch, report([[1]], truncated=True))
    with pytest.raises(PluginError) as refused:
        board.query("/b", "SELECT 1")
    assert refused.value.code == "controls::board::truncated"
    printing(monkeypatch, report([[1]], truncated=False))
    assert board.query("/b", "SELECT 1") == [[1]]
