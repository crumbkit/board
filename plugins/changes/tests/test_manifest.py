"""The committed manifest and schema are the generated ones, a call loads no pydantic, and enabling installs them."""

import json
import subprocess
import sys
from typing import Any, cast

from board_plugin.protocol import dumped
from conftest import PLUGIN, Board

from board_changes.manifest import manifest, schemas


def test_the_committed_manifest_and_schema_are_the_ones_the_models_generate() -> None:
    assert (PLUGIN / "plugin.json").read_text() == dumped(manifest())
    for path, schema in schemas().items():
        assert (PLUGIN / "types" / path).read_text() == dumped(schema), path


CALL = '{"$type": "board/call/v1", "capability": "none", "input": {}, "board": {"root": "/b"}}'


def loads_pydantic(module: str) -> bool:
    """Whether running `python -m module` on a call loads pydantic, by Python's own import log."""
    ran = subprocess.run(
        [sys.executable, "-X", "importtime", "-m", module], input=CALL, capture_output=True, text=True, check=False
    )
    return " pydantic" in ran.stderr


def test_a_call_loads_no_pydantic_and_generating_the_manifest_does() -> None:
    assert not loads_pydantic("board_changes")
    assert loads_pydantic("board_changes.manifest")


def test_board_lists_the_plugin_as_enabled_and_enabling_wrote_the_schema_it_ships(board: Board) -> None:
    report = board.json("plugin", "list")
    plugin = next(plugin for plugin in report["data"] if plugin["name"] == "changes")
    assert (plugin["status"], plugin["enabled"]) == ("ok", True)
    assert plugin["offers"] == ["start", "next", "attach", "advance", "open", "pr"]
    for path, schema in schemas().items():
        assert json.loads((board.root / ".board" / "types" / path).read_text()) == schema, path


def test_a_capability_the_plugin_does_not_offer_is_refused_and_one_it_offers_is_answered(board: Board) -> None:
    code, document = board.plugin("close", {})
    assert (code, document["code"]) == (1, "changes::call::unknown_capability")
    assert board.plugin("open", {}) == (0, {"$type": "board/call-result/v1", "output": board.call("open", {})})


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
        ("start", "repo"),
        ("start", "title"),
        ("start", "name"),
        ("start", "handle"),
        ("next", "path"),
        ("attach", "path"),
        ("attach", "ref"),
        ("attach", "route"),
        ("attach", "head"),
        ("attach", "issue"),
        ("attach", "pull_request"),
        ("advance", "path"),
        ("open", "limit"),
        ("pr", "path"),
    }
    for key, description in descriptions.items():
        assert isinstance(description, str), key
        assert description.endswith("."), key
