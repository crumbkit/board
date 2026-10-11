"""`board_controls.board`, through the real `board`."""

import os
import shlex

import pytest
from board_plugin import board as plugin_board
from conftest import Board, board_binary, control

from board_controls.board import referenced


def test_referenced_resolves_more_controls_than_one_refs_run_answers_over_two_runs(
    board: Board, monkeypatch: pytest.MonkeyPatch
) -> None:
    names = ["a", "b", "c", "d"]
    for name in names:
        board.write(f"docs/{name}.md", name)
        board.write(f"controls/{name}.json", control("Small.", applies_to=[f"/docs/{name}.md"]))
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

    def runs() -> list[str]:
        """The paths asked by each `board refs` since the last call."""
        asked = [line.split(" -- ", 1)[1] for line in log.read_text().splitlines()]
        log.unlink()
        return asked

    controls = [f"/controls/{name}" for name in names]
    expected = {f"/controls/{name}": {f"/docs/{name}.md"} for name in names}
    assert referenced(plugin_board.Board(str(board.root), "controls", paths_per_run=4), controls) == expected
    assert runs() == ["/controls/a /controls/b /controls/c /controls/d"]
    assert referenced(plugin_board.Board(str(board.root), "controls", paths_per_run=3), controls) == expected
    assert runs() == ["/controls/a /controls/b /controls/c", "/controls/d"]
