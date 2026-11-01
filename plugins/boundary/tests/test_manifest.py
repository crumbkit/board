"""The committed manifest and schemas are generated, a call loads no pydantic, and enabling installs the schemas."""

import json
import subprocess
import sys

from board_plugin.protocol import dumped
from conftest import PLUGIN, Board, boundary

from board_boundary.manifest import manifest, schemas


def test_the_committed_manifest_and_schemas_are_the_ones_the_shapes_generate() -> None:
    assert (PLUGIN / "plugin.json").read_text() == dumped(manifest())
    for name, schema in schemas().items():
        assert (PLUGIN / "types" / f"{name}.schema.json").read_text() == dumped(schema)


CALL = '{"$type": "board/call/v1", "capability": "none", "input": {}, "board": {"root": "/b"}}'


def loads_pydantic(module: str) -> bool:
    """Whether running `python -m module` on a call loads pydantic, by Python's own import log."""
    ran = subprocess.run(
        [sys.executable, "-X", "importtime", "-m", module], input=CALL, capture_output=True, text=True, check=False
    )
    return " pydantic" in ran.stderr


def test_a_call_loads_no_pydantic_and_generating_the_manifest_does() -> None:
    assert not loads_pydantic("board_boundary")
    assert loads_pydantic("board_boundary.manifest")


def skill_description() -> str:
    """The `description` of SKILL.md's frontmatter, a single-quoted YAML scalar on one line."""
    [line] = [line for line in (PLUGIN / "SKILL.md").read_text().splitlines() if line.startswith("description: ")]
    quoted = line.removeprefix("description: ")
    assert quoted.startswith("'"), quoted
    assert quoted.endswith("'"), quoted
    return quoted[1:-1].replace("''", "'")


def test_the_manifest_describes_the_plugin_with_the_skills_description() -> None:
    assert manifest()["description"] == skill_description()


def test_a_boundary_without_checks_is_invalid_and_one_with_a_check_is_not(personal: Board) -> None:
    personal.write(
        "controls/names.json", json.dumps({"$type": "controls/control/v1", "requirement": "R.", "purpose": "P."})
    )
    check = {"control": "/controls/names", "parts": ["*"]}
    personal.write("boundaries/ok.json", boundary(to="github:octo/*", actions=["github.*"], checks=[check]))
    personal.write("boundaries/bad.json", boundary(to="github:octo/*", actions=["github.*"], checks=[]))
    report = json.loads(personal.run("--json", "check").stdout)
    status = {entry["logical"]: entry for entry in report["data"]}
    assert status["/boundaries/ok"]["status"] == "ok"
    assert status["/.board/types/boundary/crossing/v1.schema"]["status"] == "ok"
    assert status["/boundaries/bad"]["code"] == "board::record::invalid"


def test_a_check_naming_a_missing_control_dangles_and_one_naming_a_control_does_not(personal: Board) -> None:
    personal.write(
        "controls/names.json", json.dumps({"$type": "controls/control/v1", "requirement": "R.", "purpose": "P."})
    )
    for name, control in (("ok", "/controls/names"), ("gone", "/controls/gone")):
        check = {"control": control, "parts": ["*"]}
        personal.write(f"boundaries/{name}.json", boundary(to="github:octo/*", actions=["*"], checks=[check]))
    report = json.loads(personal.run("--json", "check").stdout)
    status = {entry["logical"]: entry for entry in report["data"]}
    assert status["/boundaries/ok"]["status"] == "ok"
    assert status["/boundaries/gone"]["code"] == "board::ref::dangling"
