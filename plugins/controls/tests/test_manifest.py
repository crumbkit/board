"""Board lists the built-in plugin as enabled, and enabling wrote the schema the plugin ships."""

import json

from conftest import SCHEMAS, Board, control


def test_board_lists_the_plugin_as_enabled_and_enabling_wrote_the_schema_it_ships(board: Board) -> None:
    report = board.run("--json", "plugin", "list")
    plugin = next(plugin for plugin in json.loads(report.stdout)["data"] if plugin["name"] == "controls")
    assert (plugin["status"], plugin["enabled"]) == ("ok", True)
    assert "run" not in plugin
    assert sorted(path.name for path in SCHEMAS.iterdir()) == ["control", "source"]
    for name in ["control", "source"]:
        shipped = SCHEMAS / name / "v1.schema.json"
        written = board.root / ".board" / "types" / "controls" / name / "v1.schema.json"
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


def test_a_pattern_rule_with_an_unknown_show_is_invalid_and_one_with_a_known_show_is_not(board: Board) -> None:
    board.write("private/names.txt", "acme\n")
    board.write("controls/known.json", control("R.", rule={"pattern": "/private/names.txt", "show": "match-only"}))
    board.write("controls/unknown.json", control("R.", rule={"pattern": "/private/names.txt", "show": "all"}))
    report = json.loads(board.run("--json", "check").stdout)
    status = {entry["logical"]: entry for entry in report["data"]}
    assert status["/controls/known"]["status"] == "ok"
    assert status["/controls/unknown"]["code"] == "board::record::invalid"


def test_a_rule_file_board_cannot_find_dangles_and_one_it_finds_does_not(board: Board) -> None:
    board.write("rubrics/plain.md", "Write plainly.\n")
    board.write("controls/found.json", control("R.", rule={"judged": "/rubrics/plain.md"}))
    board.write("controls/lost.json", control("R.", rule={"judged": "/rubrics/lost.md"}))
    report = json.loads(board.run("--json", "check").stdout)
    status = {entry["logical"]: entry for entry in report["data"]}
    assert status["/controls/found"]["status"] == "ok"
    assert status["/controls/lost"]["code"] == "board::ref::dangling"


def test_a_rule_mixing_two_kinds_is_invalid_and_each_kind_alone_is_not(board: Board) -> None:
    board.write("rubrics/plain.md", "Write plainly.\n")
    board.write("controls/budget.json", control("R.", rule={"budget": "words", "max": 1}))
    board.write("controls/mixed.json", control("R.", rule={"budget": "words", "max": 1, "judged": "/rubrics/plain.md"}))
    board.write("controls/stray.json", control("R.", rule={"judged": "/rubrics/plain.md", "show": "pattern"}))
    board.write("controls/null.json", control("R.", rule=None))
    report = json.loads(board.run("--json", "check").stdout)
    status = {entry["logical"]: entry for entry in report["data"]}
    assert status["/controls/budget"]["status"] == "ok"
    for name in ("mixed", "stray", "null"):
        assert status[f"/controls/{name}"]["code"] == "board::record::invalid", name
