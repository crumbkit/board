"""The plugin's manifest and the schemas it ships, generated from its models (STANDARDS.md P3)."""

from typing import Any

from board_plugin.protocol import dumped
from pydantic import BaseModel, JsonValue

from board_github.models import ISSUE_SCHEMA_PATH, PULL_SCHEMA_PATH, IssueRecord, PullRecord, RefInput, RefOutput
from board_github.objects import ISSUE_TYPE, PULL_TYPE

# The variables gh reads for its account and configuration, which Board passes a capability that runs gh.
GH_ENV = ["GH_TOKEN", "GITHUB_TOKEN", "GH_CONFIG_DIR"]


def capability(
    name: str, description: str, given: type[BaseModel], output: type[BaseModel], *, read_only: bool
) -> dict[str, Any]:
    """One capability's Tool object."""
    return {
        "name": name,
        "description": description,
        "inputSchema": given.model_json_schema(),
        "outputSchema": output.model_json_schema(),
        "annotations": {"readOnlyHint": read_only},
        "_meta": {},
    }


def manifest() -> dict[str, JsonValue]:
    """`board/plugin-manifest/v1` for this plugin."""
    return {
        "$type": "board/plugin-manifest/v1",
        "name": "github",
        "version": "0.1.0",
        # SKILL.md's description, the callout an agent sees first.
        "description": (
            "Record a GitHub pull request or issue as a typed record that other records can reference. Use it "
            "when a record should name a pull request or issue. Run `board -C <board> call github.ref --ref "
            "<owner>/<repo>#<number>`, then reference the `path` it returns. SKILL.md and README.md in the "
            "plugin's directory say more."
        ),
        "run": ["uv", "run", "--frozen", "--no-dev", "python", "-m", "board_github"],
        "metadata": {"area": "delivery"},
        "types": [PULL_TYPE, ISSUE_TYPE],
        "capabilities": [
            capability(
                "ref",
                "Write the record of a pull request or issue, asking gh which it is and its title, or report the "
                "record already there without asking.",
                RefInput,
                RefOutput,
                read_only=False,
            )
            | {"env": list[JsonValue](GH_ENV)},
        ],
    }


def schemas() -> dict[str, dict[str, JsonValue]]:
    """Each type's schema, as the plugin ships it, by its path under `types/`."""
    return {
        PULL_SCHEMA_PATH: PullRecord.model_json_schema(by_alias=True),
        ISSUE_SCHEMA_PATH: IssueRecord.model_json_schema(by_alias=True),
    }


if __name__ == "__main__":
    print(dumped(manifest()), end="")  # noqa: T201 - printing the manifest is this module's job
