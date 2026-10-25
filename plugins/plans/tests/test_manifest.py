"""The committed manifest and schemas are the generated ones, a call loads no pydantic, and enabling installs them."""

import json
import os
import subprocess
import sys
from pathlib import Path
from typing import Any, cast

from board_plugin.protocol import dumped
from conftest import PLUGIN, Board, board_binary

from board_plans.manifest import manifest, schemas


def test_the_committed_manifest_and_schemas_are_the_ones_the_models_generate() -> None:
    assert (PLUGIN / "plugin.json").read_text() == dumped(manifest())
    for path, schema in schemas().items():
        assert (PLUGIN / "types" / path).read_text() == dumped(schema), path


CALL = '{"$type": "board/call/v1", "capability": "none", "input": {}, "board": {"root": "/b", "database": "/d"}}'


def loads_pydantic(module: str) -> bool:
    """Whether running `python -m module` on a call loads pydantic, by Python's own import log."""
    ran = subprocess.run(
        [sys.executable, "-X", "importtime", "-m", module], input=CALL, capture_output=True, text=True, check=False
    )
    return " pydantic" in ran.stderr


def test_a_call_loads_no_pydantic_and_generating_the_manifest_does() -> None:
    assert not loads_pydantic("board_plans")
    assert loads_pydantic("board_plans.manifest")


def test_board_lists_the_plugin_as_enabled_and_enabling_wrote_the_schemas_it_ships(board: Board) -> None:
    report = board.json("plugin", "list")
    plugin = next(plugin for plugin in report["data"] if plugin["name"] == "plans")
    assert (plugin["status"], plugin["enabled"]) == ("ok", True)
    for path, schema in schemas().items():
        assert json.loads((board.root / ".board" / "types" / path).read_text()) == schema, path


def request(root: Path, capability: str) -> dict[str, Any]:
    """A call of `capability` with no input."""
    return {
        "$type": "board/call/v1",
        "capability": capability,
        "input": {},
        "board": {"root": str(root), "database": str(root / "unused.sqlite")},
    }


def plugin(given: dict[str, Any], board: Board) -> tuple[int, dict[str, Any]]:
    """Run the plugin once on `given`, with the tested `board` first on PATH: its exit code and document."""
    path = os.pathsep.join([str(board_binary().parent), os.environ.get("PATH", "")])
    ran = subprocess.run(
        [sys.executable, "-m", "board_plans"],
        cwd=board.root,
        env={**os.environ, "PATH": path, "XDG_DATA_HOME": str(board.data), "XDG_CACHE_HOME": str(board.cache)},
        input=json.dumps(given),
        capture_output=True,
        text=True,
        check=False,
    )
    document: dict[str, Any] = json.loads(ran.stdout)
    return ran.returncode, document


def test_a_capability_the_plugin_does_not_offer_is_refused_and_one_it_offers_is_answered(board: Board) -> None:
    code, document = plugin(request(board.root, "closed"), board)
    assert (code, document["code"]) == (1, "plans::call::unknown_capability")
    code, document = plugin(request(board.root, "open"), board)
    assert (code, document["$type"]) == (0, "board/call-result/v1")


def test_every_input_property_carries_a_sentence_that_says_what_it_means() -> None:
    capabilities = cast("list[dict[str, Any]]", manifest()["capabilities"])
    descriptions = {
        (capability["name"], name): field.get("description")
        for capability in capabilities
        for name, field in capability["inputSchema"]["properties"].items()
    }
    assert set(descriptions) == {("open", "path"), ("open", "limit")}
    for key, description in descriptions.items():
        assert isinstance(description, str), key
        assert description.endswith("."), key
