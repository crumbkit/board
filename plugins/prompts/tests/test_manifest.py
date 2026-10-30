"""The committed manifest and schema are generated, a call loads no pydantic, and enabling installs the schema."""

import json
import subprocess
import sys
from typing import Any, cast

from board_plugin.protocol import dumped
from conftest import PLUGIN, Board

from board_prompts.manifest import manifest, schemas


def test_the_committed_manifest_and_schema_are_the_ones_the_models_generate() -> None:
    assert (PLUGIN / "plugin.json").read_text() == dumped(manifest())
    for path, schema in schemas().items():
        assert (PLUGIN / "types" / path).read_text() == dumped(schema), path


CALL = '{"$type": "board/call/v1", "capability": "none", "input": {}, "board": {"root": "/b"}}'


def loads_pydantic(module: str) -> tuple[bool, str]:
    """Whether running `python -m module` on a call loads pydantic, by Python's own import log, and its stdout."""
    ran = subprocess.run(
        [sys.executable, "-X", "importtime", "-m", module], input=CALL, capture_output=True, text=True, check=False
    )
    return " pydantic" in ran.stderr, ran.stdout


def test_a_call_loads_no_pydantic_and_generating_the_manifest_does() -> None:
    loaded, stdout = loads_pydantic("board_prompts")
    assert not loaded
    assert json.loads(stdout)["code"] == "prompts::call::unknown_capability"
    assert loads_pydantic("board_prompts.manifest")[0]


def test_board_lists_the_plugin_as_enabled_and_enabling_wrote_the_schemas_it_ships(board: Board) -> None:
    report = json.loads(board.run("--json", "plugin", "list").stdout)
    listed = next(plugin for plugin in report["data"] if plugin["name"] == "prompts")
    assert (listed["status"], listed["enabled"], listed["offers"]) == ("ok", True, ["get", "list"])
    for path, schema in schemas().items():
        assert json.loads((board.root / ".board" / "types" / path).read_text()) == schema, path


def skill_description() -> str:
    """The `description` of SKILL.md's frontmatter, a single-quoted YAML scalar on one line."""
    [line] = [line for line in (PLUGIN / "SKILL.md").read_text().splitlines() if line.startswith("description: ")]
    quoted = line.removeprefix("description: ")
    assert quoted.startswith("'"), quoted
    assert quoted.endswith("'"), quoted
    return quoted[1:-1].replace("''", "'")


def test_the_manifest_describes_the_plugin_with_the_skills_description() -> None:
    assert manifest()["description"] == skill_description()


def test_every_input_property_carries_a_sentence_that_says_what_it_means() -> None:
    capabilities = cast("list[dict[str, Any]]", manifest()["capabilities"])
    descriptions = {
        (capability["name"], name): field.get("description")
        for capability in capabilities
        for name, field in capability["inputSchema"]["properties"].items()
    }
    assert set(descriptions) == {
        ("get", "path"),
        ("get", "inputs"),
        ("get", "runs_in"),
        ("list", "repo"),
        ("list", "remote"),
    }
    for key, description in descriptions.items():
        assert isinstance(description, str), key
        assert description.endswith("."), key
