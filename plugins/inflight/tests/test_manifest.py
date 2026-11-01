"""The committed manifest is generated, a call loads no pydantic, and Board lists the plugin as enabled."""

import json
import subprocess
import sys
from typing import Any, cast

from board_plugin.protocol import dumped
from conftest import PLUGIN, Board

from board_inflight.manifest import manifest


def test_the_committed_manifest_is_the_one_the_models_generate() -> None:
    assert (PLUGIN / "plugin.json").read_text() == dumped(manifest())


CALL = '{"$type": "board/call/v1", "capability": "none", "input": {}, "board": {"root": "/b"}}'


def loads_pydantic(module: str) -> tuple[bool, str]:
    """Whether running `python -m module` on a call loads pydantic, by Python's own import log, and its stdout."""
    ran = subprocess.run(
        [sys.executable, "-X", "importtime", "-m", module], input=CALL, capture_output=True, text=True, check=False
    )
    return " pydantic" in ran.stderr, ran.stdout


def test_a_call_loads_no_pydantic_and_generating_the_manifest_does() -> None:
    loaded, stdout = loads_pydantic("board_inflight")
    assert not loaded
    assert json.loads(stdout)["code"] == "inflight::call::unknown_capability"
    assert loads_pydantic("board_inflight.manifest")[0]


def test_board_lists_the_plugin_as_enabled(board: Board) -> None:
    report = json.loads(board.run("--json", "plugin", "list").stdout)
    listed = next(plugin for plugin in report["data"] if plugin["name"] == "inflight")
    assert (listed["status"], listed["enabled"], listed["offers"]) == ("ok", True, ["now"])


def test_every_input_property_carries_a_sentence_that_says_what_it_means() -> None:
    capabilities = cast("list[dict[str, Any]]", manifest()["capabilities"])
    properties = capabilities[0]["inputSchema"]["properties"]
    assert set(properties) == {"holder", "stopped-at", "limit"}
    for name, field in properties.items():
        assert str(field.get("description", "")).endswith("."), name
