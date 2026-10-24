"""The plugin's manifest, generated from its models (STANDARDS.md P3)."""

from board_plugin.protocol import dumped
from pydantic import JsonValue

from board_notes.models import AddInput, AddOutput, DailyInput, DailyOutput


def manifest() -> dict[str, JsonValue]:
    """`board/plugin-manifest/v1` for this plugin."""
    return {
        "$type": "board/plugin-manifest/v1",
        "name": "notes",
        "version": "0.1.0",
        # SKILL.md's description, the callout an agent sees first.
        "description": (
            "Append a note to today's daily note, or to a topic's note, in a Reflect vault mounted at "
            "/vaults/<name>, and read a day's daily note. Use it when the person asks you to note something down or "
            "asks what their daily note says. Start with `board -C <board> call notes.daily`, which syncs the vault "
            "and prints today's note. SKILL.md and README.md in the plugin's directory say more."
        ),
        "run": ["uv", "run", "--frozen", "--no-dev", "python", "-m", "board_notes"],
        "metadata": {"area": "notes"},
        "capabilities": [
            {
                "name": "add",
                "description": (
                    "Sync the vault, append text under a `## HH:MM` heading to today's daily note or a topic's note,"
                    " commit it through the mount and push it."
                ),
                "inputSchema": AddInput.model_json_schema(),
                "outputSchema": AddOutput.model_json_schema(),
                "_meta": {},
            },
            {
                "name": "daily",
                "description": "Sync the vault and print a day's daily note, unless the note is private.",
                "inputSchema": DailyInput.model_json_schema(),
                "outputSchema": DailyOutput.model_json_schema(),
                "_meta": {},
            },
        ],
    }


if __name__ == "__main__":
    print(dumped(manifest()), end="")  # noqa: T201 - printing the manifest is this module's job
