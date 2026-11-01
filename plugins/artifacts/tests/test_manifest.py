"""The committed manifest and schemas are the generated ones, and enabling the plugin installs the schemas."""

import json
import os
import subprocess
import sys
from pathlib import Path
from typing import Any, cast

from board_plugin.protocol import dumped
from conftest import PLUGIN, SCHEMAS, Board, board_binary

from board_artifacts.manifest import manifest
from board_artifacts.models import ArtifactRecord, VersionRecord, schema_of


def test_the_committed_manifest_and_schemas_are_the_ones_the_models_generate() -> None:
    assert (PLUGIN / "plugin.json").read_text() == dumped(manifest())
    assert (SCHEMAS / "artifact" / "v1.schema.json").read_text() == dumped(schema_of(ArtifactRecord))
    assert (SCHEMAS / "version" / "v1.schema.json").read_text() == dumped(schema_of(VersionRecord))


def skill_description() -> str:
    """The `description` of SKILL.md's frontmatter, a single-quoted YAML scalar on one line."""
    [line] = [line for line in (PLUGIN / "SKILL.md").read_text().splitlines() if line.startswith("description: ")]
    quoted = line.removeprefix("description: ")
    assert quoted.startswith("'"), quoted
    assert quoted.endswith("'"), quoted
    return quoted[1:-1].replace("''", "'")


def test_the_manifest_describes_the_plugin_with_the_skills_description() -> None:
    assert manifest()["description"] == skill_description()


def test_board_lists_the_plugin_as_enabled_and_enabling_wrote_the_schemas_it_ships(board: Board) -> None:
    report = board.json("plugin", "list")
    plugin = next(plugin for plugin in report["data"] if plugin["name"] == "artifacts")
    assert plugin["status"] == "ok"
    assert plugin["enabled"] is True
    types = board.root / ".board" / "types" / "artifacts"
    assert json.loads((types / "artifact" / "v1.schema.json").read_text()) == schema_of(ArtifactRecord)
    assert json.loads((types / "version" / "v1.schema.json").read_text()) == schema_of(VersionRecord)


def request(root: Path, capability: str) -> dict[str, Any]:
    """A call of `capability` with no input."""
    return {"$type": "board/call/v1", "capability": capability, "input": {}, "board": {"root": str(root)}}


def plugin(given: dict[str, Any], board: Board) -> tuple[int, dict[str, Any]]:
    """Run the plugin once on `given`, with the tested `board` first on PATH: its exit code and document."""
    path = os.pathsep.join([str(board_binary().parent), os.environ.get("PATH", "")])
    ran = subprocess.run(
        [sys.executable, "-m", "board_artifacts"],
        cwd=board.root,
        env={
            **os.environ,
            "PATH": path,
            "XDG_DATA_HOME": str(board.data),
            "XDG_CACHE_HOME": str(board.cache),
            "XDG_CONFIG_HOME": str(board.cache.parent / "config"),
        },
        input=json.dumps(given),
        capture_output=True,
        text=True,
        check=False,
    )
    document: dict[str, Any] = json.loads(ran.stdout)
    return ran.returncode, document


def test_a_capability_the_plugin_does_not_offer_is_refused_and_one_it_offers_is_answered(board: Board) -> None:
    code, document = plugin(request(board.root, "show"), board)
    assert (code, document["code"]) == (1, "artifacts::call::unknown_capability")
    code, document = plugin(request(board.root, "list"), board)
    assert (code, document["$type"]) == (0, "board/call-result/v1")


def test_every_input_property_carries_a_sentence_that_says_what_it_means() -> None:
    capabilities = cast("list[dict[str, Any]]", manifest()["capabilities"])
    descriptions = {
        (capability["name"], name): field.get("description")
        for capability in capabilities
        for name, field in capability["inputSchema"]["properties"].items()
    }
    assert set(descriptions) == {("list", "account"), ("list", "limit"), ("import", "account"), ("import", "listing")}
    for key, description in descriptions.items():
        assert isinstance(description, str), key
        assert description.endswith("."), key
