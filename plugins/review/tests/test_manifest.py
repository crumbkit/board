"""The committed manifest and schemas are the generated ones, a call loads no pydantic, and Board lists the plugin."""

import json
import subprocess
import sys
from typing import Any, cast

from board_plugin.protocol import dumped
from conftest import PLUGIN, Repo

from board_review.manifest import manifest, schemas


def test_the_committed_manifest_and_schemas_are_the_ones_the_shapes_generate() -> None:
    assert (PLUGIN / "plugin.json").read_text() == dumped(manifest())
    for path, schema in schemas().items():
        assert (PLUGIN / path).read_text() == dumped(schema), path


def request(capability: str) -> str:
    """A call of `capability` against a board that is not there."""
    board = {"root": "/no-such-board"}
    return json.dumps({"$type": "board/call/v1", "capability": capability, "input": {}, "board": board})


def ran(module: str, capability: str = "none") -> subprocess.CompletedProcess[str]:
    """`python -X importtime -m module` given a call of `capability`."""
    return subprocess.run(
        [sys.executable, "-X", "importtime", "-m", module],
        input=request(capability),
        capture_output=True,
        text=True,
        check=False,
    )


def test_a_call_loads_no_pydantic_and_generating_the_manifest_does() -> None:
    assert " pydantic" not in ran("board_review").stderr
    assert " pydantic" in ran("board_review.manifest").stderr


def test_an_unknown_capability_is_refused_and_a_known_one_is_dispatched() -> None:
    unknown = json.loads(ran("board_review", "none").stdout)
    assert unknown["code"] == "review::call::unknown_capability"
    known = json.loads(ran("board_review", "covered").stdout)
    assert known["code"] != "review::call::unknown_capability"


def test_every_input_property_carries_a_sentence_that_says_what_it_means() -> None:
    capabilities = cast("list[dict[str, Any]]", manifest()["capabilities"])
    descriptions = {
        (capability["name"], name): field.get("description")
        for capability in capabilities
        for name, field in capability["inputSchema"]["properties"].items()
    }
    assert set(descriptions) == {
        ("run", "brief"),
        ("post", "report"),
        ("post", "pr"),
        ("covered", "base"),
        ("covered", "head"),
    }
    for key, description in descriptions.items():
        assert isinstance(description, str), key
        assert description.endswith("."), key


def test_board_lists_the_plugin_as_working_with_its_types_installed(repo: Repo) -> None:
    listed = json.loads(repo.run("--json", "plugin", "list").stdout)
    [review] = [plugin for plugin in listed["data"] if plugin["name"] == "review"]
    assert review["status"] == "ok", review
    checked = json.loads(repo.run("--json", "check").stdout)
    status = {entry["logical"]: entry["status"] for entry in checked["data"]}
    assert status["/.board/types/review/report/v1.schema"] == "ok"


def test_post_reads_only_ghs_credentials_and_has_a_minute() -> None:
    capabilities = cast("list[dict[str, Any]]", manifest()["capabilities"])
    [post] = [capability for capability in capabilities if capability["name"] == "post"]
    assert post["env"] == ["GH_TOKEN", "GITHUB_TOKEN", "GH_CONFIG_DIR"]
    assert post["timeout"] == 60  # noqa: PLR2004 - the manifest's figure


def test_a_runs_output_names_a_range_or_a_subject_as_a_brief_does() -> None:
    capabilities = cast("list[dict[str, Any]]", manifest()["capabilities"])
    [run] = [capability for capability in capabilities if capability["name"] == "run"]
    brief = schemas()["types/review/brief/v1.schema.json"]
    assert run["outputSchema"]["oneOf"] == brief["oneOf"] == [{"required": ["range"]}, {"required": ["subject"]}]
