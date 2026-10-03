"""The committed manifest is the generated one, and Board accepts it."""

from conftest import PLUGIN, Board

from board_tasks.manifest import text


def test_the_committed_manifest_is_the_one_the_models_generate() -> None:
    assert (PLUGIN / "plugin.json").read_text() == text()


def test_board_lists_the_plugin_as_installed_enabled_and_offering_due(board: Board) -> None:
    report = board.json("plugin", "list")
    tasks = next(plugin for plugin in report["plugins"] if plugin["name"] == "tasks")
    assert tasks["status"] == "ok"
    assert tasks["enabled"] is True
