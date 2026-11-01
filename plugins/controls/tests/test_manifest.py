"""The committed manifest and schema are the generated ones, and enabling the plugin installs the schema."""

import json
import subprocess
import sys
from typing import Any, cast

from board_plugin.protocol import dumped
from conftest import PLUGIN, Board, control

from board_controls.manifest import manifest, schema


def test_the_committed_manifest_and_schema_are_the_ones_the_shapes_generate() -> None:
    assert (PLUGIN / "plugin.json").read_text() == dumped(manifest())
    assert (PLUGIN / "types/controls/control/v1.schema.json").read_text() == dumped(schema())


CALL = '{"$type": "board/call/v1", "capability": "none", "input": {}, "board": {"root": "/b"}}'


def loads_pydantic(module: str) -> bool:
    """Whether running `python -m module` on a call loads pydantic, by Python's own import log."""
    ran = subprocess.run(
        [sys.executable, "-X", "importtime", "-m", module], input=CALL, capture_output=True, text=True, check=False
    )
    return " pydantic" in ran.stderr


def test_a_call_loads_no_pydantic_and_generating_the_manifest_does() -> None:
    assert not loads_pydantic("board_controls")
    assert loads_pydantic("board_controls.manifest")


def test_enabling_writes_the_schema_so_a_control_checks_clean_and_a_broken_one_does_not(board: Board) -> None:
    board.write("controls/ok.json", control("R."))
    board.write("controls/bad.json", '{"$type": "controls/control/v1", "requirement": "R."}')
    report = json.loads(board.run("--json", "check").stdout)
    status = {entry["logical"]: entry for entry in report["data"]}
    assert status["/controls/ok"]["status"] == "ok"
    assert status["/.board/types/controls/control/v1.schema"]["status"] == "ok"
    assert status["/controls/bad"]["status"] != "ok"
    assert status["/controls/bad"]["code"] == "board::record::invalid"


def skill_description() -> str:
    """The `description` of SKILL.md's frontmatter, a single-quoted YAML scalar on one line."""
    [line] = [line for line in (PLUGIN / "SKILL.md").read_text().splitlines() if line.startswith("description: ")]
    quoted = line.removeprefix("description: ")
    assert quoted.startswith("'"), quoted
    assert quoted.endswith("'"), quoted
    return quoted[1:-1].replace("''", "'")


def test_the_manifest_describes_the_plugin_with_the_skills_description() -> None:
    assert manifest()["description"] == skill_description()


def input_descriptions() -> dict[tuple[str, str], object]:
    """Each capability's input properties, by capability and property name, with their `description`."""
    capabilities = cast("list[dict[str, Any]]", manifest()["capabilities"])
    return {
        (capability["name"], name): field.get("description")
        for capability in capabilities
        for name, field in capability["inputSchema"]["properties"].items()
    }


def test_every_input_property_carries_a_sentence_that_says_what_it_means() -> None:
    descriptions = input_descriptions()
    assert set(descriptions) == {
        ("check", "path"),
        ("for-path", "path"),
        ("evaluate", "controls"),
        ("evaluate", "parts"),
    }
    for key, description in descriptions.items():
        assert isinstance(description, str), key
        assert description.endswith("."), key


def test_call_help_prints_the_path_flag_with_its_sentence(board: Board) -> None:
    sentence = str(input_descriptions()["check", "path"]).removesuffix(".")
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
