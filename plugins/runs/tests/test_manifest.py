"""Board lists the built-in plugin as enabled, and enabling wrote the schemas the plugin ships."""

from conftest import SCHEMAS, Board


def test_board_lists_the_plugin_as_enabled_and_enabling_wrote_the_schemas_it_ships(board: Board) -> None:
    report = board.json("plugin", "list")
    plugin = next(plugin for plugin in report["data"] if plugin["name"] == "runs")
    assert (plugin["status"], plugin["enabled"]) == ("ok", True)
    assert "run" not in plugin
    assert sorted(path.name for path in SCHEMAS.iterdir()) == ["run", "suite"]
    types = board.root / ".board" / "types" / "runs"
    for name in ("suite", "run"):
        written = types / name / "v1.schema.json"
        assert written.read_text() == (SCHEMAS / name / "v1.schema.json").read_text()


def test_call_help_prints_each_flag_of_trend_with_its_sentence(board: Board) -> None:
    ran = board.run("call", "runs.trend", "-h")
    assert ran.returncode == 0, ran.stderr
    for flag in ("--suite <suite>", "--measure <measure>", "--case <case>", "--limit <limit>"):
        assert flag in ran.stdout, ran.stdout
    assert "The measure's name, such as median_ms." in ran.stdout, ran.stdout
