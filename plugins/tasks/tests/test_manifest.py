"""The committed manifest and schema are the generated ones, and enabling the plugin installs the schema."""

import json
from typing import Any, cast

from board_plugin.protocol import dumped
from conftest import PLUGIN, SCHEMA, Board

from board_tasks.manifest import manifest, schema, text


def test_the_committed_manifest_and_schema_are_the_ones_the_models_generate() -> None:
    assert (PLUGIN / "plugin.json").read_text() == text()
    assert SCHEMA.read_text() == dumped(schema())


def test_board_lists_the_plugin_as_enabled_and_enabling_wrote_the_schema_it_ships(board: Board) -> None:
    report = board.json("plugin", "list")
    tasks = next(plugin for plugin in report["data"] if plugin["name"] == "tasks")
    assert tasks["status"] == "ok"
    assert tasks["enabled"] is True
    assert json.loads((board.root / ".board/types/tasks/task/v1.schema.json").read_text()) == schema()


def test_every_input_property_carries_a_sentence_that_says_what_it_means() -> None:
    capabilities = cast("list[dict[str, Any]]", manifest()["capabilities"])
    descriptions = {
        (capability["name"], name): field.get("description")
        for capability in capabilities
        for name, field in capability["inputSchema"]["properties"].items()
    }
    assert set(descriptions) == {("due", "before"), ("next", "limit")}
    for key, description in descriptions.items():
        assert isinstance(description, str), key
        assert description.endswith("."), key
