"""The plugin's manifest and the schemas it ships, generated from its models (STANDARDS.md P3)."""

from board_plugin.protocol import dumped
from pydantic import JsonValue

from board_artifacts.models import (
    ARTIFACT_TYPE,
    VERSION_TYPE,
    ImportInput,
    ImportOutput,
    ListInput,
    ListOutput,
    schema_of,
)

DESCRIPTION = (
    "The claude.ai artifacts the owner publishes, each with a copy of every version's page, recorded by the"
    " board-artifacts Claude Code mod. Use it to find an artifact, or to sync the artifacts published outside Claude"
    " Code. Start with `board --personal call artifacts.list`, which lists the artifacts with their newest version,"
    " newest first. SKILL.md and README.md in the plugin's directory say more."
)


def manifest() -> dict[str, JsonValue]:
    """`board/plugin-manifest/v1` for this plugin."""
    return {
        "$type": "board/plugin-manifest/v1",
        "name": "artifacts",
        "version": "0.1.0",
        "description": DESCRIPTION,
        "run": ["uv", "run", "--frozen", "--no-dev", "python", "-m", "board_artifacts"],
        "metadata": {"area": "publishing"},
        "types": [ARTIFACT_TYPE, VERSION_TYPE],
        "capabilities": [
            {
                "name": "list",
                "description": "The artifacts in the board with their newest version and its title, newest first.",
                "inputSchema": schema_of(ListInput),
                "outputSchema": schema_of(ListOutput),
                "annotations": {"readOnlyHint": True},
                "_meta": {},
            },
            {
                "name": "import",
                "description": (
                    "Write an artifact record, without a version, for each artifact of an Artifact tool listing"
                    " that the board does not have."
                ),
                "inputSchema": schema_of(ImportInput),
                "outputSchema": schema_of(ImportOutput),
                "_meta": {},
            },
        ],
    }


if __name__ == "__main__":
    print(dumped(manifest()), end="")  # noqa: T201 - printing the manifest is this module's job
