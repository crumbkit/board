"""The committed manifest and schema are the generated ones, and enabling the plugin installs the schema."""

import json

from board_plugin.protocol import dumped
from conftest import PLUGIN, SCHEMA, Board

from board_tasks.manifest import schema, text


def test_the_committed_manifest_and_schema_are_the_ones_the_models_generate() -> None:
    assert (PLUGIN / "plugin.json").read_text() == text()
    assert SCHEMA.read_text() == dumped(schema())


def test_board_lists_the_plugin_as_enabled_and_enabling_wrote_the_schema_it_ships(board: Board) -> None:
    report = board.json("plugin", "list")
    tasks = next(plugin for plugin in report["plugins"] if plugin["name"] == "tasks")
    assert tasks["status"] == "ok"
    assert tasks["enabled"] is True
    assert json.loads((board.root / ".board/types/tasks/task/v1.schema.json").read_text()) == schema()
