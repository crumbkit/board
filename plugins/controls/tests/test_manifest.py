"""The committed manifest and schema are the generated ones, and enabling the plugin installs the schema."""

import json
import subprocess
import sys

from board_plugin.protocol import dumped
from conftest import PLUGIN, Board, control

from board_controls.manifest import manifest, schema


def test_the_committed_manifest_and_schema_are_the_ones_the_shapes_generate() -> None:
    assert (PLUGIN / "plugin.json").read_text() == dumped(manifest())
    assert (PLUGIN / "types/controls/control/v1.schema.json").read_text() == dumped(schema())


CALL = '{"$type": "board/call/v1", "capability": "none", "input": {}, "board": {"root": "/b", "database": "/d"}}'


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
