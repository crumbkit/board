"""The plugin's manifest, generated from its models (STANDARDS.md P3)."""

from board_plugin.protocol import dumped
from pydantic import JsonValue

from board_share.models import ToInput, ToOutput


def manifest() -> dict[str, JsonValue]:
    """`board/plugin-manifest/v1` for this plugin."""
    return {
        "$type": "board/plugin-manifest/v1",
        "name": "share",
        "version": "0.1.0",
        # SKILL.md's description, the callout an agent sees first.
        "description": (
            "Copy a record from this board into a mounted board, such as a team's, with its children, every "
            "record and file it references and the schemas of their types, so a reader there can open it. Use "
            "it when someone else needs to read a record you wrote. Start with `board -C <board> call share.to "
            "--path /<record> --to /<mount> --dry_run true`, which lists what would be copied. SKILL.md and "
            "README.md in the plugin's directory say more."
        ),
        "run": ["uv", "run", "--frozen", "--no-dev", "python", "-m", "board_share"],
        "metadata": {"area": "sharing"},
        "capabilities": [
            {
                "name": "to",
                "description": (
                    "Copy a record, its children, everything they reference and the schemas it lacks into a"
                    " mounted board, as one commit there, and move the mount's pin to it."
                ),
                "inputSchema": ToInput.model_json_schema(),
                "outputSchema": ToOutput.model_json_schema(),
                "_meta": {},
            },
        ],
    }


if __name__ == "__main__":
    print(dumped(manifest()), end="")  # noqa: T201 - printing the manifest is this module's job
