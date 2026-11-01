"""Board lists the built-in plugin as enabled, and enabling wrote the schema the plugin ships."""

import json

from conftest import SCHEMAS, Board, control


def test_board_lists_the_plugin_as_enabled_and_enabling_wrote_the_schema_it_ships(board: Board) -> None:
    report = board.run("--json", "plugin", "list")
    plugin = next(plugin for plugin in json.loads(report.stdout)["data"] if plugin["name"] == "controls")
    assert (plugin["status"], plugin["enabled"]) == ("ok", True)
    assert "run" not in plugin
    assert sorted(path.name for path in SCHEMAS.iterdir()) == ["control"]
    shipped = SCHEMAS / "control" / "v1.schema.json"
    written = board.root / ".board" / "types" / "controls" / "control" / "v1.schema.json"
    assert written.read_text() == shipped.read_text()


def test_enabling_writes_the_schema_so_a_control_checks_clean_and_a_broken_one_does_not(board: Board) -> None:
    board.write("controls/ok.json", control("R."))
    board.write("controls/bad.json", '{"$type": "controls/control/v1", "requirement": "R."}')
    report = json.loads(board.run("--json", "check").stdout)
    status = {entry["logical"]: entry for entry in report["data"]}
    assert status["/controls/ok"]["status"] == "ok"
    assert status["/.board/types/controls/control/v1.schema"]["status"] == "ok"
    assert status["/controls/bad"]["status"] != "ok"
    assert status["/controls/bad"]["code"] == "board::record::invalid"


def test_call_help_prints_the_path_flag_with_its_sentence(board: Board) -> None:
    sentence = "The directory or file whose controls to check, by its logical path, such as /docs; / when left out"
    relative = "; a path without a leading / is read from the current directory."
    ran = board.run("call", "controls.check", "-h")
    assert ran.returncode == 0, ran.stderr
    assert f"--path <path>  {sentence}{relative}\n" in ran.stdout, ran.stdout


def test_a_budget_measured_per_directory_is_invalid_and_one_measured_per_file_is_not(board: Board) -> None:
    board.write("controls/file.json", control("R.", rule={"budget": "words", "max": 1, "per": "file"}))
    board.write("controls/directory.json", control("R.", rule={"budget": "words", "max": 1, "per": "directory"}))
    report = json.loads(board.run("--json", "check").stdout)
    status = {entry["logical"]: entry for entry in report["data"]}
    assert status["/controls/file"]["status"] == "ok"
    assert status["/controls/directory"]["code"] == "board::record::invalid"
