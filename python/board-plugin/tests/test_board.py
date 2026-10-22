"""How the library reads what `board` prints, against a fake `board` and the real one."""

import json
import os
import subprocess
from pathlib import Path
from typing import Any, cast

import pytest

from board_plugin import board
from board_plugin.board import Board
from board_plugin.protocol import PluginError

REPOSITORY = Path(__file__).resolve().parents[3]
DEMO = Board("/b", "demo")


def printing(monkeypatch: pytest.MonkeyPatch, stdout: bytes, code: int = 0, stderr: bytes = b"logs") -> None:
    """Make every `board` the library runs print `stdout` and `stderr` and exit with `code`."""

    def run(_self: Board, *arguments: str, **_: bytes) -> subprocess.CompletedProcess[bytes]:
        return subprocess.CompletedProcess(["board", *arguments], code, stdout, stderr)

    monkeypatch.setattr(Board, "_run", run)


def report(rows: list[list[Any]], *, truncated: bool, plugins: list[dict[str, str]] | None = None) -> bytes:
    """A `board/query-report/v2` holding `rows`, and `plugins` that failed."""
    document = {
        "schema": "board/query-report/v2",
        "data": rows,
        "truncated": truncated,
        "summary": {"plugins": plugins or []},
    }
    return json.dumps(document).encode()


def test_a_board_that_prints_no_json_is_a_failure_and_one_that_does_is_read(monkeypatch: pytest.MonkeyPatch) -> None:
    printing(monkeypatch, b"Usage: board [OPTIONS]")
    with pytest.raises(PluginError) as refused:
        DEMO.rows("SELECT 1")
    assert refused.value.code == "demo::board::failed"
    assert "first on PATH" in refused.value.message
    printing(monkeypatch, report([[1]], truncated=False))
    assert DEMO.rows("SELECT 1") == [[1]]


def test_a_board_error_is_a_refusal_carrying_boards_code_fields_and_hint(monkeypatch: pytest.MonkeyPatch) -> None:
    error: dict[str, Any] = {
        "schema": "board/failure/v1",
        "code": "board::query::invalid",
        "message": "no such table",
        "recovery": {"kind": "hint", "text": "fix it"},
    }
    printing(monkeypatch, json.dumps(error).encode(), code=1)
    with pytest.raises(PluginError) as refused:
        DEMO.rows("SELECT 1")
    assert refused.value.code == "demo::board::refused"
    assert refused.value.fields == {"code": "board::query::invalid"}
    refusals = [{"line": 1, "code": "board::mount::stale", "message": "/team/a cannot be written"}]
    printing(
        monkeypatch, json.dumps({**error, "fields": {"refusals": refusals, "code": "not Board's"}}).encode(), code=1
    )
    with pytest.raises(PluginError) as refused:
        DEMO.rows("SELECT 1")
    assert refused.value.fields == {"refusals": refusals, "code": "board::query::invalid"}
    assert refused.value.message == "`board query` refused with board::query::invalid: no such table; fix it"
    error["recovery"] = {"kind": "run", "call": "board query -h"}
    printing(monkeypatch, json.dumps(error).encode(), code=1)
    with pytest.raises(PluginError) as refused:
        DEMO.rows("SELECT 1")
    assert refused.value.message.endswith("no such table; run `board query -h`")


def test_a_failed_board_without_an_error_document_is_a_failure(monkeypatch: pytest.MonkeyPatch) -> None:
    printing(monkeypatch, json.dumps({"schema": "board/query-report/v2"}).encode(), code=1)
    with pytest.raises(PluginError) as refused:
        DEMO.rows("SELECT 1")
    assert refused.value.code == "demo::board::failed"


def test_rows_asks_board_for_every_row_and_query_for_at_most_its_limit(monkeypatch: pytest.MonkeyPatch) -> None:
    asked: list[tuple[str, ...]] = []

    def run(_self: Board, *arguments: str, **_: bytes) -> subprocess.CompletedProcess[bytes]:
        asked.append(arguments)
        return subprocess.CompletedProcess(["board", *arguments], 0, report([[1]], truncated=False), b"")

    monkeypatch.setattr(Board, "_run", run)
    assert DEMO.rows("SELECT 1") == [[1]]
    assert DEMO.query("SELECT 1") == ([[1]], False)
    assert DEMO.query("SELECT 1", 2) == ([[1]], False)
    assert asked == [
        ("--json", "query", "--all", "SELECT 1"),
        ("--json", "query", "--limit", "10000", "SELECT 1"),
        ("--json", "query", "--limit", "2", "SELECT 1"),
    ]


def test_rows_refuses_a_query_cut_at_boards_byte_budget_and_query_returns_it_as_truncated(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    summary: dict[str, Any] = {"plugins": [], "code": board.OVER_BUDGET}
    over = {"schema": "board/query-report/v2", "data": [[1]], "truncated": True, "summary": summary}
    printing(monkeypatch, json.dumps(over).encode())
    assert DEMO.query("SELECT 1") == ([[1]], True)
    with pytest.raises(PluginError) as refused:
        DEMO.rows("SELECT 1")
    assert refused.value.code == "demo::board::over_budget"
    assert "ORDER BY" in refused.value.message
    printing(monkeypatch, json.dumps({**over, "truncated": False}).encode())
    assert DEMO.rows("SELECT 1") == [[1]]


def batch(command: str, asked: tuple[str, ...] | list[str], answers: list[Any]) -> bytes:
    """A `board/<command>-report/v2` answering each of `asked` as crumbkit prints a batch.

    An answer with a `code` is a refused path, in `failures` with its index.
    The first answer whose code is an over-budget code leaves that path and
    every one after it out, as one failure without an index.
    """
    data: list[Any] = []
    failures: list[dict[str, Any]] = []
    for index, answer in enumerate(answers):
        code = cast("dict[str, Any]", answer).get("code") if isinstance(answer, dict) else None
        if code == board.OVER_BUDGET:
            fields = {"left_out": list(asked[index:]), "left_out_from": index, "budget": 64}
            failures.append({"code": code, "message": "not answered", "fields": fields})
            break
        if code is None:
            data.append(answer)
        else:
            failures.append({**answer, "key": asked[index], "index": index})
    return json.dumps({"schema": f"board/{command}-report/v2", "data": data, "failures": failures}).encode()


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
    without = {"schema": "board/query-report/v2", "data": [[1]], "truncated": False}
    printing(monkeypatch, json.dumps(without).encode())
    with pytest.raises(PluginError) as refused:
        DEMO.rows("SELECT 1")
    assert refused.value.code == "demo::board::failed"
    printing(monkeypatch, json.dumps({**without, "summary": {"plugins": []}}).encode())
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
        document = {"schema": "board/refs-report/v2", "data": answers, "failures": []}
        printing(monkeypatch, json.dumps(document).encode())
        with pytest.raises(PluginError) as refused:
            DEMO.refs(paths)
        assert refused.value.code == "demo::board::failed"
        assert "printed a report without one answer per path" in refused.value.message
        assert refused.value.fields == {"stderr": "logs"}
    printing(monkeypatch, batch("refs", ["/a", "/b"], [answer, answer]))
    assert DEMO.refs(["/a", "/b"]) == [answer, answer]


def test_refs_of_no_paths_starts_no_run_and_of_one_path_starts_one_that_reads_it_after_dashes(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    runs: list[tuple[str, ...]] = []
    answer = {"location": {"display": "/-a"}}
    report = batch("refs", ["-a"], [answer])

    def run(_self: Board, *arguments: str, **_: bytes) -> subprocess.CompletedProcess[bytes]:
        runs.append(arguments)
        return subprocess.CompletedProcess(["board", *arguments], 0, report, b"")

    monkeypatch.setattr(Board, "_run", run)
    assert DEMO.refs([]) == []
    assert runs == []
    assert DEMO.refs(["-a"], missing=True) == [answer]
    assert runs == [("--json", "refs", "--all", "--missing", "--", "-a")]


def answering_each_path(monkeypatch: pytest.MonkeyPatch) -> list[tuple[str, ...]]:
    """Make `board refs` answer each path after `--` with its display path, and return the runs it records."""
    runs: list[tuple[str, ...]] = []

    def run(_self: Board, *arguments: str, **_: bytes) -> subprocess.CompletedProcess[bytes]:
        runs.append(arguments)
        asked = arguments[arguments.index("--") + 1 :]
        report = batch("refs", asked, [{"location": {"display": path}} for path in asked])
        return subprocess.CompletedProcess(["board", *arguments], 0, report, b"")

    monkeypatch.setattr(Board, "_run", run)
    return runs


def test_refs_asks_as_many_paths_as_one_run_answers_in_one_run_and_one_more_in_a_second(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    runs = answering_each_path(monkeypatch)
    three = Board("/b", "demo", paths_per_run=3)
    assert [answer and answer["location"]["display"] for answer in three.refs(["/a", "/b", "/c"])] == ["/a", "/b", "/c"]
    assert [run[4:] for run in runs] == [("/a", "/b", "/c")]
    runs.clear()
    answers = three.refs(["/a", "/b", "/c", "/d"])
    assert [answer and answer["location"]["display"] for answer in answers] == ["/a", "/b", "/c", "/d"]
    assert [run[4:] for run in runs] == [("/a", "/b", "/c"), ("/d",)]


def test_refs_of_one_str_is_refused_and_of_a_list_holding_it_is_read(monkeypatch: pytest.MonkeyPatch) -> None:
    runs = answering_each_path(monkeypatch)
    with pytest.raises(TypeError, match=r"not the one str '/ab'; pass \['/ab'\]"):
        DEMO.refs("/ab")
    assert runs == []
    assert DEMO.refs(["/ab"]) == [{"location": {"display": "/ab"}}]


def answering_within(monkeypatch: pytest.MonkeyPatch, budget: int) -> list[tuple[str, ...]]:
    """Make `board refs` answer each path until `budget` paths, then over budget, and return each run's arguments."""
    runs: list[tuple[str, ...]] = []

    def run(_self: Board, *arguments: str, **_: bytes) -> subprocess.CompletedProcess[bytes]:
        runs.append(arguments)
        asked = arguments[arguments.index("--") + 1 :]
        over = {"code": board.OVER_BUDGET}
        answers = [{"location": {"display": path}} if at < budget else over for at, path in enumerate(asked)]
        report = batch("refs", asked, answers)
        return subprocess.CompletedProcess(["board", *arguments], 1 if budget < len(asked) else 0, report, b"")

    monkeypatch.setattr(Board, "_run", run)
    return runs


def test_refs_asks_again_for_the_paths_a_run_left_over_its_budget_with_the_same_flags(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    paths = ["/a", "/b", "/c", "/d"]
    runs = answering_within(monkeypatch, 4)
    assert [answer and answer["location"]["display"] for answer in DEMO.refs(paths)] == paths
    assert [run[run.index("--") + 1 :] for run in runs] == [("/a", "/b", "/c", "/d")]
    runs = answering_within(monkeypatch, 2)
    answers = DEMO.refs(paths, fields=["/status", "/$type"])
    assert [answer and answer["location"]["display"] for answer in answers] == paths
    flags = ("--json", "refs", "--all", "--field", "/status", "--field", "/$type", "--")
    assert runs == [(*flags, "/a", "/b", "/c", "/d"), (*flags, "/c", "/d")]


def test_refs_fails_on_a_run_that_answers_none_of_its_paths_and_reads_one_that_answers_one(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    runs = answering_within(monkeypatch, 0)
    with pytest.raises(PluginError) as failed:
        DEMO.refs(["/a", "/b"])
    assert failed.value.code == "demo::board::failed"
    assert f"left every path unanswered as {board.OVER_BUDGET}" in failed.value.message
    assert [run[-2:] for run in runs] == [("/a", "/b")]
    runs = answering_within(monkeypatch, 1)
    assert [answer and answer["location"]["display"] for answer in DEMO.refs(["/a", "/b"])] == ["/a", "/b"]
    assert [run[run.index("--") + 1 :] for run in runs] == [("/a", "/b"), ("/b",)]


def test_a_get_report_without_one_readable_answer_per_path_is_a_failure_keeping_stderr_and_one_with_it_is_read(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    text, raw = {"text": "one two"}, {"base64": "/wBh"}
    for paths, answers in (
        (["/a", "/b"], {"a": text}),
        (["/a", "/b"], [text]),
        (["/a"], [text, text]),
        (["/a", "/b"], [text, "/b"]),
        (["/a"], [{"location": {"display": "/a"}}]),
        (["/a"], [{"base64": "not base64!"}]),
    ):
        document = {"schema": "board/get-report/v2", "data": answers, "failures": []}
        printing(monkeypatch, json.dumps(document).encode())
        with pytest.raises(PluginError) as refused:
            DEMO.get(paths)
        assert refused.value.code == "demo::board::failed"
        assert "printed a report without one file or refusal per path" in refused.value.message
        assert refused.value.fields == {"stderr": "logs"}
    printing(monkeypatch, batch("get", ["/a", "/b"], [text, raw]))
    assert DEMO.get(["/a", "/b"]) == [b"one two", b"\xff\x00a"]


def test_get_of_no_paths_starts_no_run_and_of_one_path_starts_one_that_reads_it_after_dashes(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    runs: list[tuple[str, ...]] = []
    report = batch("get", ["-a"], [{"text": "x"}])

    def run(_self: Board, *arguments: str, **_: bytes) -> subprocess.CompletedProcess[bytes]:
        runs.append(arguments)
        return subprocess.CompletedProcess(["board", *arguments], 0, report, b"")

    monkeypatch.setattr(Board, "_run", run)
    assert DEMO.get([]) == []
    assert runs == []
    assert DEMO.get(["-a"]) == [b"x"]
    assert runs == [("--json", "get", "--", "-a")]


def getting_within(monkeypatch: pytest.MonkeyPatch, budget: int) -> list[tuple[str, ...]]:
    """Make `board get` answer each path with its own name as text until `budget` paths, then over budget.

    It returns the paths each run was asked.
    """
    runs: list[tuple[str, ...]] = []

    def run(_self: Board, *arguments: str, **_: bytes) -> subprocess.CompletedProcess[bytes]:
        asked = arguments[arguments.index("--") + 1 :]
        runs.append(asked)
        over = {"code": board.OVER_BUDGET}
        answers = [{"text": path} if at < budget else over for at, path in enumerate(asked)]
        report = batch("get", asked, answers)
        return subprocess.CompletedProcess(["board", *arguments], 1 if budget < len(asked) else 0, report, b"")

    monkeypatch.setattr(Board, "_run", run)
    return runs


def test_get_asks_again_for_the_paths_a_run_left_over_its_budget_and_keeps_their_order(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    paths = ["/a", "/b", "/c", "/d"]
    runs = getting_within(monkeypatch, 4)
    assert DEMO.get(paths) == [b"/a", b"/b", b"/c", b"/d"]
    assert runs == [("/a", "/b", "/c", "/d")]
    runs = getting_within(monkeypatch, 2)
    assert DEMO.get(paths) == [b"/a", b"/b", b"/c", b"/d"]
    assert runs == [("/a", "/b", "/c", "/d"), ("/c", "/d")]


def test_get_fails_on_a_run_that_answers_none_of_its_paths_and_reads_one_that_answers_one(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    runs = getting_within(monkeypatch, 0)
    with pytest.raises(PluginError) as failed:
        DEMO.get(["/a", "/b"])
    assert failed.value.code == "demo::board::failed"
    assert f"left every path unread as {board.OVER_BUDGET}" in failed.value.message
    assert runs == [("/a", "/b")]
    runs = getting_within(monkeypatch, 1)
    assert DEMO.get(["/a", "/b"]) == [b"/a", b"/b"]
    assert runs == [("/a", "/b"), ("/b",)]


def test_get_asks_as_many_paths_as_one_run_answers_in_one_run_and_one_more_in_a_second(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    three = Board("/b", "demo", paths_per_run=3)
    runs = getting_within(monkeypatch, 10)
    assert three.get(["/a", "/b", "/c"]) == [b"/a", b"/b", b"/c"]
    assert runs == [("/a", "/b", "/c")]
    runs.clear()
    assert three.get(["/a", "/b", "/c", "/d"]) == [b"/a", b"/b", b"/c", b"/d"]
    assert runs == [("/a", "/b", "/c"), ("/d",)]


def test_get_of_one_str_is_refused_and_of_a_list_holding_it_is_read(monkeypatch: pytest.MonkeyPatch) -> None:
    runs = getting_within(monkeypatch, 10)
    with pytest.raises(TypeError, match=r"not the one str '/ab'; pass \['/ab'\]"):
        DEMO.get("/ab")
    assert runs == []
    assert DEMO.get(["/ab"]) == [b"/ab"]


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
    subprocess.run(["git", "config", "user.name", "t"], cwd=root, check=True)
    subprocess.run(["git", "config", "user.email", "t@t"], cwd=root, check=True)
    subprocess.run([binary, "init"], cwd=root, check=True, capture_output=True)
    (root / "docs").mkdir()
    (root / "docs" / "a.md").write_text("one two")
    return Board(str(root), "demo")


def test_one_run_asks_at_most_as_many_paths_as_the_built_board_says_one_run_takes(real: Board) -> None:
    for command in ("refs", "get"):
        ran = subprocess.run(["board", command, "--help"], cwd=real.root, capture_output=True, check=True, text=True)
        assert f"At most {board.PATHS_PER_RUN} paths" in " ".join(ran.stdout.split())
    assert DEMO.paths_per_run == board.PATHS_PER_RUN


def test_the_real_board_gives_each_files_bytes_in_order_and_a_refused_path_its_code(real: Board) -> None:
    (Path(real.root) / "docs" / "raw.bin").write_bytes(b"\xff\x00a")
    answers = real.get(["/docs/a.md", "/docs", "/docs/raw.bin"])
    assert answers[0] == b"one two"
    assert answers[2] == b"\xff\x00a"
    refused = answers[1]
    assert isinstance(refused, PluginError)
    assert refused.code == "demo::board::refused"
    assert refused.fields == {"path": "/docs", "code": "board::record::not_a_record"}
    assert real.get(["/docs/a.md", "/docs/raw.bin"]) == [b"one two", b"\xff\x00a"]


def test_the_real_board_cuts_a_run_at_its_byte_budget_with_the_code_the_library_asks_again_for(
    real: Board, monkeypatch: pytest.MonkeyPatch
) -> None:
    for name in "bcd":
        (Path(real.root) / "docs" / f"{name}.md").write_text(name)
    paths = [f"/docs/{name}.md" for name in "abcd"]
    monkeypatch.setenv("BOARD_OUTPUT_BUDGET", "300")
    ran = subprocess.run(["board", "--json", "get", *paths], cwd=real.root, capture_output=True, check=False)
    assert json.loads(ran.stdout)["failures"][-1]["code"] == board.OVER_BUDGET
    assert real.get(paths) == [b"one two", b"b", b"c", b"d"]
    assert [answer and answer["location"]["display"] for answer in real.refs(paths)] == paths


def test_a_document_run_writes_its_stdin_to_board_and_one_without_stdin_writes_nothing(real: Board) -> None:
    written = real.document("put", "--file", "/docs/b.md", stdin=b"three")
    assert written["data"]["writes"][0]["location"]["display"] == "/docs/b.md"
    assert real.get(["/docs/b.md"]) == [b"three"]
    with pytest.raises(PluginError) as refused:
        real.document("put", "/docs/c")
    assert refused.value.fields == {"path": "/docs/c", "code": "board::json::parse"}


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


def test_the_real_board_gives_each_references_target_values_and_codes(real: Board) -> None:
    root = Path(real.root)
    (root / ".board" / "types" / "x" / "item").mkdir(parents=True)
    schema = {
        "type": "object",
        "properties": {"see": {"type": "array", "items": {"type": "string", "format": "board-ref"}}},
    }
    (root / ".board" / "types" / "x" / "item" / "v1.schema.json").write_text(json.dumps(schema))
    (root / "b.json").write_text('{"$type": "x/item/v1", "status": "done"}')
    (root / "a.json").write_text('{"$type": "x/item/v1", "see": ["/b", "/docs/a.md"]}')
    [answer] = real.refs(["/a"], fields=["/status", "/nope"])
    assert answer is not None
    done, note = answer["from"]
    assert done["values"] == [{"pointer": "/status", "value": "done"}, {"pointer": "/nope", "absent": True}]
    assert "target_code" not in done
    assert "values" not in note
    assert note["target_code"] == "board::record::not_a_record"
