"""Board lists the built-in plugin as enabled, and enabling wrote the schemas the plugin ships."""

import json

from conftest import REPOSITORY, Board

SCHEMAS = REPOSITORY / "crates" / "board-plugin-plans" / "types"


def test_board_lists_the_plugin_as_enabled_and_enabling_wrote_the_schemas_it_ships(board: Board) -> None:
    report = board.json("plugin", "list")
    plugin = next(plugin for plugin in report["data"] if plugin["name"] == "plans")
    assert (plugin["status"], plugin["enabled"]) == ("ok", True)
    shipped = sorted(SCHEMAS.glob("plans/*/v1.schema.json"))
    assert len(shipped) == 2  # noqa: PLR2004 - question and answer
    for path in shipped:
        written = board.root / ".board" / "types" / path.relative_to(SCHEMAS)
        assert json.loads(written.read_text()) == json.loads(path.read_text()), path
