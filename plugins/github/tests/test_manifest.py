"""The committed manifest and schemas are the generated ones, a call loads no pydantic, and enabling installs them."""

import json
import os
import subprocess
import sys
from typing import Any, cast

from board_plugin.protocol import dumped
from conftest import PLUGIN, Board, board_binary

from board_github.manifest import manifest, schemas


def test_the_committed_manifest_and_schemas_are_the_ones_the_models_generate() -> None:
    assert (PLUGIN / "plugin.json").read_text() == dumped(manifest())
    for path, schema in schemas().items():
        assert (PLUGIN / "types" / path).read_text() == dumped(schema), path


RECORD = {
    "$type": "github/issue/v1",
    "owner": "octo",
    "repo": "widgets",
    "number": 7,
    "url": "https://github.com/octo/widgets/issues/7",
}
CALL = '{"$type": "board/call/v1", "capability": "none", "input": {}, "board": {"root": "/b"}}'


def loads_pydantic(module: str) -> bool:
    """Whether running `python -m module` on a call loads pydantic, by Python's own import log."""
    ran = subprocess.run(
        [sys.executable, "-X", "importtime", "-m", module], input=CALL, capture_output=True, text=True, check=False
    )
    return " pydantic" in ran.stderr


def test_a_call_loads_no_pydantic_and_generating_the_manifest_does() -> None:
    assert not loads_pydantic("board_github")
    assert loads_pydantic("board_github.manifest")


def test_board_lists_the_plugin_as_enabled_and_enabling_wrote_the_schemas_it_ships(board: Board) -> None:
    report = board.json("plugin", "list")
    plugin = next(plugin for plugin in report["data"] if plugin["name"] == "github")
    assert (plugin["status"], plugin["enabled"]) == ("ok", True)
    assert plugin["offers"] == ["ref", "push", "pr-create", "pr-edit", "comment", "issue-create"]
    for path, schema in schemas().items():
        assert json.loads((board.root / ".board" / "types" / path).read_text()) == schema, path


def test_a_capability_the_plugin_does_not_offer_is_refused_and_one_it_offers_is_answered(board: Board) -> None:
    board.write("gh/octo/widgets/issues/7.json", json.dumps(RECORD))

    def served(capability: str) -> tuple[int, dict[str, Any]]:
        call = {"$type": "board/call/v1", "capability": capability, "input": {"ref": "octo/widgets#7"}}
        path = os.pathsep.join([str(board_binary().parent), os.environ.get("PATH", "")])
        ran = subprocess.run(
            [sys.executable, "-m", "board_github"],
            cwd=board.root,
            env={**os.environ, "PATH": path, "XDG_DATA_HOME": str(board.data), "XDG_CACHE_HOME": str(board.cache)},
            input=json.dumps(call | {"board": {"root": str(board.root)}}),
            capture_output=True,
            text=True,
            check=False,
        )
        return ran.returncode, json.loads(ran.stdout)

    code, document = served("pull")
    assert (code, document["code"]) == (1, "github::call::unknown_capability")
    assert served("ref") == (0, {"$type": "board/call-result/v1", "output": board.call("octo/widgets#7")})


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
        ("ref", "ref"),
        ("pr-create", "title"),
        ("pr-create", "body"),
        ("pr-create", "base"),
        ("pr-create", "draft"),
        ("pr-edit", "number"),
        ("pr-edit", "title"),
        ("pr-edit", "body"),
        ("comment", "number"),
        ("comment", "body"),
        ("issue-create", "title"),
        ("issue-create", "body"),
    }
    for key, description in descriptions.items():
        assert isinstance(description, str), key
        assert description.endswith("."), key
