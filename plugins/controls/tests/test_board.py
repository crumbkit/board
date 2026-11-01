"""`board_controls.board`, through the real `board`."""

import json
import os
import shlex
import subprocess
from pathlib import Path
from typing import Any

import pytest
from board_plugin import board as plugin_board
from board_plugin.protocol import PluginError
from conftest import Board, board_binary, control, library

from board_controls.board import controls, matched, matching, referenced


def logged(board: Board, monkeypatch: pytest.MonkeyPatch) -> Path:
    """Make every `board` this process runs write its arguments as one line to the returned log, then run."""
    log = board.root.parent / "calls.log"
    wrapper = board.root.parent / "bin"
    wrapper.mkdir()
    (wrapper / "board").write_text(
        f'#!/bin/sh\necho "$*" >> {shlex.quote(str(log))}\nexec {shlex.quote(str(board_binary()))} "$@"\n'
    )
    (wrapper / "board").chmod(0o755)
    monkeypatch.setenv("PATH", f"{wrapper}{os.pathsep}{os.environ['PATH']}")
    monkeypatch.setenv("XDG_DATA_HOME", str(board.data))
    monkeypatch.setenv("XDG_CACHE_HOME", str(board.cache))
    monkeypatch.setenv("XDG_CONFIG_HOME", str(board.cache.parent / "config"))
    return log


def test_referenced_resolves_more_controls_than_one_refs_run_answers_over_two_runs(
    board: Board, monkeypatch: pytest.MonkeyPatch
) -> None:
    names = ["a", "b", "c", "d"]
    for name in names:
        board.write(f"docs/{name}.md", name)
        board.write(f"controls/{name}.json", control("Small.", applies_to=[f"/docs/{name}.md"]))
    found = controls(plugin_board.Board(str(board.root), "controls"))
    log = logged(board, monkeypatch)

    def runs() -> list[str]:
        """The paths asked by each `board refs` since the last call."""
        asked = [line.split(" -- ", 1)[1] for line in log.read_text().splitlines()]
        log.unlink()
        return asked

    expected = {f"/controls/{name}": {f"/docs/{name}.md"} for name in names}
    assert referenced(plugin_board.Board(str(board.root), "controls", paths_per_run=4), found) == expected
    assert runs() == ["/controls/a /controls/b /controls/c /controls/d"]
    assert referenced(plugin_board.Board(str(board.root), "controls", paths_per_run=3), found) == expected
    assert runs() == ["/controls/a /controls/b /controls/c", "/controls/d"]


def queries(log: Path) -> int:
    """How many `board query` runs the log holds."""
    return sum(" query " in f" {line} " for line in log.read_text().splitlines())


FIVE = [
    ["/controls/a", "/docs/1.md", "ok", None],
    ["/controls/a", "/docs/2.md", "ok", None],
    ["/controls/a", "/docs/3.md", "ok", None],
    ["/controls/b", "/notes/1.md", "ok", None],
    ["/controls/b", "/notes/2.md", "ok", None],
]


@pytest.mark.parametrize(("page", "runs"), [(2, 3), (5, 1)])
def test_matched_lists_every_pair_once_over_as_many_pages_as_it_needs(
    board: Board, monkeypatch: pytest.MonkeyPatch, page: int, runs: int
) -> None:
    for name in ["1", "2", "3"]:
        board.write(f"docs/{name}.md", name)
    for name in ["1", "2"]:
        board.write(f"notes/{name}.md", name)
    board.write("controls/a.json", control("Small.", paths=["docs/*.md"]))
    board.write("controls/b.json", control("Small.", paths=["notes/*", "notes/1.md"]))
    log = logged(board, monkeypatch)
    assert matched(plugin_board.Board(str(board.root), "controls"), page=page) == FIVE
    assert queries(log) == runs


def test_matched_pages_past_paths_and_patterns_that_hold_quotes_and_backslashes(
    board: Board, monkeypatch: pytest.MonkeyPatch
) -> None:
    files = ["it's.md", 'say "hi".md', "back\\slash.md", "café [1].md"]
    for name in files:
        board.write(f"docs/{name}", name)
    board.write("controls/o'brien.json", control("Small.", paths=["docs/it's*", 'docs/*"*', "docs/*\\*", "docs/caf*"]))
    log = logged(board, monkeypatch)
    rows = matched(plugin_board.Board(str(board.root), "controls"), page=1)
    assert rows == [["/controls/o'brien", f"/docs/{name}", "ok", None] for name in sorted(files)]
    assert queries(log) == len(files)


def test_matched_pages_across_mounts_and_a_pattern_anchored_in_the_own_tree_stays_there(
    board: Board, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    inner = library(tmp_path / "inner", {"docs/1.md": "1"})
    board.write("vendor/lib.json", library(tmp_path / "outer", {"docs/1.md": "1", "inner.json": inner}))
    board.write("docs/1.md", "1")
    board.write("controls/a.json", control("Small.", paths=["*1.md"]))
    board.write("controls/b.json", control("Small.", paths=["docs/*"]))
    log = logged(board, monkeypatch)
    rows = matched(plugin_board.Board(str(board.root), "controls"), page=1)
    assert [row[:2] for row in rows] == [
        ["/controls/a", "/docs/1.md"],
        ["/controls/a", "/vendor/lib/docs/1.md"],
        ["/controls/a", "/vendor/lib/inner/docs/1.md"],
        ["/controls/b", "/docs/1.md"],
    ]
    assert queries(log) == len(rows)


def test_matched_parses_each_control_once_and_lists_each_file_once(board: Board) -> None:
    board.write("docs/1.md", "1")
    board.write("controls/a.json", control("Small.", paths=["docs/*"]))
    ran = board.run("--json", "query", f"EXPLAIN QUERY PLAN {matching(('', '', ''))}")
    plan = {row[0]: row for row in json.loads(ran.stdout)["data"]}

    def within(row: list[Any]) -> str:
        """The step `row` runs in, as the plan names it, or nothing at the top."""
        return plan[row[1]][3] if row[1] in plan else ""

    scans = {row[3]: within(row) for row in plan.values()}
    assert scans["SCAN p VIRTUAL TABLE INDEX 3:"] == "MATERIALIZE patterns"
    assert "MATERIALIZE files" in scans.values()


def test_matched_names_a_symlink_named_json_and_a_json_file_by_their_file_names(
    board: Board, monkeypatch: pytest.MonkeyPatch
) -> None:
    board.write("docs/t.md", "T.\n")
    board.write("docs/a.json", "{}")
    (board.root / "docs/l.json").symlink_to("t.md")
    board.write("controls/names.json", control("R.", paths=["docs/l.json", "docs/a.json"]))
    board.write("controls/doubled.json", control("R.", paths=["*.json.json"]))
    logged(board, monkeypatch)
    rows = matched(plugin_board.Board(str(board.root), "controls"))
    assert [row[:2] for row in rows] == [["/controls/names", "/docs/a"], ["/controls/names", "/docs/l"]]


@pytest.mark.parametrize(
    "rows", [[], [["/controls/a", "/docs/1.md", "ok", None, "/controls/a", "docs/1.md", "worktree"]]]
)
def test_a_truncated_page_that_ends_no_later_than_the_last_fails_and_one_that_is_complete_is_read(
    monkeypatch: pytest.MonkeyPatch, rows: list[list[Any]]
) -> None:
    def printing(*, truncated: bool) -> None:
        document: dict[str, Any] = {
            "schema": "board/query-report/v2",
            "data": rows,
            "truncated": truncated,
            "summary": {"plugins": []},
        }

        def run(_self: plugin_board.Board, *arguments: str, **_: bytes) -> subprocess.CompletedProcess[bytes]:
            return subprocess.CompletedProcess(["board", *arguments], 0, json.dumps(document).encode(), b"")

        monkeypatch.setattr(plugin_board.Board, "_run", run)

    printing(truncated=True)
    with pytest.raises(PluginError) as refused:
        matched(plugin_board.Board("/b", "controls"))
    assert refused.value.code == "controls::board::failed"
    printing(truncated=False)
    assert matched(plugin_board.Board("/b", "controls")) == [row[:4] for row in rows]
