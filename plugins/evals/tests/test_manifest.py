"""Board lists the built-in plugin as enabled, and enabling wrote the schema the plugin ships."""

import json

from conftest import SCHEMAS, Board


def test_board_lists_the_plugin_as_enabled_and_enabling_wrote_the_schemas_it_ships(board: Board) -> None:
    report = board.json("plugin", "list")
    plugin = next(plugin for plugin in report["data"] if plugin["name"] == "evals")
    assert (plugin["status"], plugin["enabled"]) == ("ok", True)
    assert "run" not in plugin
    assert sorted(path.name for path in SCHEMAS.iterdir()) == ["eval"]
    shipped = SCHEMAS / "eval" / "v1.schema.json"
    written = board.root / ".board" / "types" / "evals" / "eval" / "v1.schema.json"
    assert json.loads(written.read_text()) == json.loads(shipped.read_text())
