"""The committed manifest and schema are generated, a call loads no pydantic, and the plugin is described once."""

import json
import os
import subprocess
import sys
from typing import Any, cast

from board_plugin.protocol import dumped
from conftest import PLUGIN, SCHEMA, Notes, board_binary

from board_notes.manifest import manifest, schema


def test_the_committed_manifest_and_schema_are_the_ones_the_models_generate() -> None:
    assert (PLUGIN / "plugin.json").read_text() == dumped(manifest())
    assert SCHEMA.read_text() == dumped(schema())


CALL = '{"$type": "board/call/v1", "capability": "none", "input": {}, "board": {"root": "/b", "database": "/d"}}'


def loads_pydantic(module: str) -> bool:
    """Whether running `python -m module` on a call loads pydantic, by Python's own import log."""
    ran = subprocess.run(
        [sys.executable, "-X", "importtime", "-m", module], input=CALL, capture_output=True, text=True, check=False
    )
    return " pydantic" in ran.stderr


def test_a_call_loads_no_pydantic_and_generating_the_manifest_does() -> None:
    assert not loads_pydantic("board_notes")
    assert loads_pydantic("board_notes.manifest")


def plugin(notes: Notes, capability: str) -> tuple[int, str]:
    """Run the plugin once, as Board would from the board: its exit code and its error code or `$type`."""
    request = {
        "$type": "board/call/v1",
        "capability": capability,
        "input": {},
        "board": {"root": str(notes.board), "database": str(notes.cache / "unused.sqlite")},
    }
    path = os.pathsep.join([str(board_binary().parent), os.environ.get("PATH", "")])
    environment = {
        **os.environ,
        "PATH": path,
        "XDG_DATA_HOME": str(notes.data),
        "XDG_CACHE_HOME": str(notes.cache),
        "XDG_CONFIG_HOME": str(notes.cache.parent / "config"),
    }
    ran = subprocess.run(
        [sys.executable, "-m", "board_notes"],
        cwd=notes.board,
        env=environment,
        input=json.dumps(request),
        capture_output=True,
        text=True,
        check=False,
    )
    document = json.loads(ran.stdout)
    return ran.returncode, document.get("code", document["$type"])


def test_a_capability_the_plugin_does_not_offer_is_refused_and_one_it_offers_is_answered(notes: Notes) -> None:
    assert plugin(notes, "find") == (1, "notes::call::unknown_capability")
    assert plugin(notes, "daily") == (0, "board/call-result/v1")


def test_board_lists_the_plugin_as_enabled_and_working(notes: Notes) -> None:
    report = json.loads(notes.run("--json", "plugin", "list").stdout)
    listed = next(plugin for plugin in report["data"] if plugin["name"] == "notes")
    assert (listed["status"], listed["enabled"], listed["offers"]) == ("ok", True, ["add", "daily"])


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
        ("add", "text"),
        ("add", "repo"),
        ("add", "remote"),
        ("add", "topic"),
        ("daily", "repo"),
        ("daily", "remote"),
        ("daily", "date"),
    }
    for key, description in descriptions.items():
        assert isinstance(description, str), key
        assert description.endswith("."), key
