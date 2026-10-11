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


def report(rows: list[list[Any]], *, truncated: bool, plugins: list[dict[str, str]] | None = None) -> bytes:
    """A `board/query-report/v1` holding `rows`, and `plugins` that failed."""
    document = {"$type": "board/query-report/v1", "rows": rows, "truncated": truncated, "plugins": plugins or []}
    return json.dumps(document).encode()


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


def failed(plugin: str, code: str) -> bytes:
    """A report of one row, listing `plugin` as failed with `code`."""
    return report([[1]], truncated=False, plugins=[{"plugin": plugin, "code": code, "message": "it exited 3"}])


def test_a_query_reporting_this_plugins_extraction_failed_is_refused_and_another_plugins_failure_is_not(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    printing(monkeypatch, failed("demo", "board::plugin::failed"))
    with pytest.raises(PluginError) as refused:
        DEMO.rows("SELECT 1")
    assert refused.value.code == "demo::board::refused"
    assert refused.value.fields == {"code": "board::plugin::failed"}
    assert refused.value.message == (
        "`board query` refused with board::plugin::failed:"
        " demo could not extract, so its rows may be stale: it exited 3"
    )
    printing(monkeypatch, failed("other", "board::plugin::failed"))
    assert DEMO.rows("SELECT 1") == [[1]]


def test_a_query_that_could_not_attach_this_plugins_database_is_read_and_its_extraction_failure_is_refused(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    printing(monkeypatch, failed("demo", "board::plugin::no_database"))
    assert DEMO.rows("SELECT 1") == [[1]]
    printing(monkeypatch, failed("demo", "board::plugin::timeout"))
    with pytest.raises(PluginError) as refused:
        DEMO.rows("SELECT 1")
    assert (refused.value.code, refused.value.fields) == ("demo::board::refused", {"code": "board::plugin::timeout"})


def test_a_query_report_without_a_list_of_plugins_is_a_failure_and_one_with_it_is_read(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    without = {"$type": "board/query-report/v1", "rows": [[1]], "truncated": False}
    printing(monkeypatch, json.dumps(without).encode())
    with pytest.raises(PluginError) as refused:
        DEMO.rows("SELECT 1")
    assert refused.value.code == "demo::board::failed"
    printing(monkeypatch, json.dumps({**without, "plugins": []}).encode())
    assert DEMO.rows("SELECT 1") == [[1]]


def test_a_refs_report_without_one_dict_answer_per_path_is_a_failure_keeping_stderr_and_one_with_it_is_read(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    answer = {"location": {"display": "/a"}}
    for paths, answers in (
        (["/a", "/b"], {"a": answer}),
        (["/a", "/b"], [answer]),
        (["/a"], [answer, answer]),
        (["/a", "/b"], [answer, "/b"]),
    ):
        printing(monkeypatch, json.dumps({"$type": "board/refs-report/v1", "answers": answers}).encode())
        with pytest.raises(PluginError) as refused:
            DEMO.refs(paths)
        assert refused.value.code == "demo::board::failed"
        assert "printed a report without one answer per path" in refused.value.message
        assert refused.value.fields == {"stderr": "logs"}
    printing(monkeypatch, json.dumps({"$type": "board/refs-report/v1", "answers": [answer, answer]}).encode())
    assert DEMO.refs(["/a", "/b"]) == [answer, answer]


def test_refs_of_no_paths_starts_no_run_and_of_one_path_starts_one_that_reads_it_after_dashes(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    runs: list[tuple[str, ...]] = []
    answer = {"location": {"display": "/-a"}}
    report = json.dumps({"$type": "board/refs-report/v1", "answers": [answer]}).encode()

    def run(_self: Board, *arguments: str) -> subprocess.CompletedProcess[bytes]:
        runs.append(arguments)
        return subprocess.CompletedProcess(["board", *arguments], 0, report, b"")

    monkeypatch.setattr(Board, "_run", run)
    assert DEMO.refs([]) == []
    assert runs == []
    assert DEMO.refs(["-a"], missing=True) == [answer]
    assert runs == [("--json", "refs", "--all", "--missing", "--", "-a")]


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
    [found] = real.refs(["/docs/a.md"])
    assert found is not None
    assert found["location"]["display"] == "/docs/a.md"
    assert real.refs(["/docs/gone.md"]) == [None]
    [missing] = real.refs(["/docs/gone.md"], missing=True)
    assert missing is not None
    assert missing["missing"]["path"] == "docs/gone.md"


def test_the_real_board_answers_several_paths_in_order_with_none_where_nothing_is(real: Board) -> None:
    (Path(real.root) / "docs" / "b.md").write_text("three")
    answers = real.refs(["/docs/b.md", "/docs/gone.md", "/docs/a.md"])
    assert [answer and answer["location"]["display"] for answer in answers] == ["/docs/b.md", None, "/docs/a.md"]
    assert real.refs(["/docs/b.md", "/docs/a.md"]) == real.refs(["/docs/b.md"]) + real.refs(["/docs/a.md"])
    located = real.refs(["/docs/a.md", "/docs/gone.md"], missing=True)
    assert located[1] is not None
    assert located[1]["missing"]["path"] == "docs/gone.md"


def test_several_paths_with_one_refused_for_another_reason_raise_its_code_and_without_it_are_answered(
    real: Board,
) -> None:
    (Path(real.root) / "gone.json").write_text('{"$type": "board/mount/v1", "path": "/elsewhere"}')
    with pytest.raises(PluginError) as refused:
        real.refs(["/docs/a.md", "/gone/new.md"], missing=True)
    assert refused.value.code == "demo::board::refused"
    assert refused.value.fields["code"] == "board::ref::dangling"
    (Path(real.root) / "elsewhere").mkdir()
    (Path(real.root) / "elsewhere" / "kept.md").write_text("text")
    answers = real.refs(["/docs/a.md", "/gone/new.md"], missing=True)
    assert answers[1] is not None
    assert answers[1]["missing"]["path"] == "elsewhere/new.md"
