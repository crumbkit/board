"""The committed manifest and schemas are the generated ones, and enabling the plugin installs the schemas."""

import json
import os
import subprocess
import sys
from pathlib import Path
from typing import Any

from board_plugin.protocol import dumped
from conftest import PLUGIN, SCHEMAS, Board, board_binary

from board_evals.manifest import eval_schema, manifest, run_schema


def test_the_committed_manifest_and_schemas_are_the_ones_the_models_generate() -> None:
    assert (PLUGIN / "plugin.json").read_text() == dumped(manifest())
    assert (SCHEMAS / "eval" / "v1.schema.json").read_text() == dumped(eval_schema())
    assert (SCHEMAS / "run" / "v1.schema.json").read_text() == dumped(run_schema())


def test_board_lists_the_plugin_as_enabled_and_enabling_wrote_the_schemas_it_ships(board: Board) -> None:
    report = board.json("plugin", "list")
    plugin = next(plugin for plugin in report["plugins"] if plugin["name"] == "evals")
    assert plugin["status"] == "ok"
    assert plugin["enabled"] is True
    types = board.root / ".board" / "types" / "evals"
    assert json.loads((types / "eval" / "v1.schema.json").read_text()) == eval_schema()
    assert json.loads((types / "run" / "v1.schema.json").read_text()) == run_schema()


def request(root: Path, capability: str) -> dict[str, Any]:
    return {
        "$type": "board/call/v1",
        "capability": capability,
        "input": {},
        "board": {"root": str(root), "database": str(root / "unused.sqlite")},
    }


def plugin(given: dict[str, Any], cwd: Path, board: Board | None = None) -> tuple[int, dict[str, Any]]:
    """Run the plugin once on `given`, with the tested `board` first on PATH: its exit code and document.

    With `board`, the plugin runs `board` with that fixture's plugins and cache.
    """
    path = os.pathsep.join([str(board_binary().parent), os.environ.get("PATH", "")])
    ran = subprocess.run(
        [sys.executable, "-m", "board_evals"],
        cwd=cwd,
        env={
            **os.environ,
            "PATH": path,
            **({"XDG_DATA_HOME": str(board.data), "XDG_CACHE_HOME": str(board.cache)} if board else {}),
        },
        input=json.dumps(given),
        capture_output=True,
        text=True,
        check=False,
    )
    document: dict[str, Any] = json.loads(ran.stdout)
    return ran.returncode, document


def test_a_capability_the_plugin_does_not_offer_is_refused_and_one_it_offers_is_answered(board: Board) -> None:
    code, document = plugin(request(board.root, "runs"), board.root)
    assert (code, document["code"]) == (1, "evals::call::unknown_capability")
    code, document = plugin(request(board.root, "open"), board.root, board)
    assert (code, document["$type"]) == (0, "board/call-result/v1")


def test_a_request_board_would_not_send_is_refused(board: Board) -> None:
    given = request(board.root, "open")
    given["colour"] = "blue"
    code, document = plugin(given, board.root, board)
    assert (code, document["code"]) == (1, "evals::protocol::bad_request")
    del given["colour"]
    assert plugin(given, board.root, board)[0] == 0


def test_a_board_that_cannot_be_read_is_reported_with_the_plugins_code_and_a_readable_one_answers(
    board: Board, tmp_path: Path
) -> None:
    code, document = plugin(request(tmp_path / "nowhere", "open"), tmp_path)
    assert (code, document["code"]) == (1, "evals::board::failed")
    (tmp_path / "plain").mkdir()
    code, document = plugin(request(tmp_path / "plain", "open"), tmp_path)
    assert (code, document["code"]) == (1, "evals::board::refused")
    assert document["fields"] == {"code": "board::config::not_found"}
    code, document = plugin(request(board.root, "open"), board.root, board)
    assert (code, document["$type"]) == (0, "board/call-result/v1")
