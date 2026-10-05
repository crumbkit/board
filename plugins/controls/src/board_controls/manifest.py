"""The plugin's manifest and the schema it ships, generated from its models (STANDARDS.md P3)."""

from board_plugin.protocol import dumped
from pydantic import JsonValue

from board_controls.models import (
    CONTROL_TYPE,
    CheckInput,
    CheckOutput,
    ControlRecord,
    ForPathInput,
    ForPathOutput,
)


def manifest() -> dict[str, JsonValue]:
    """`board/plugin-manifest/v1` for this plugin."""
    return {
        "$type": "board/plugin-manifest/v1",
        "name": "controls",
        "version": "0.1.0",
        "description": "Requirements on a board's files, stored as records in the board.",
        "run": ["uv", "run", "--frozen", "--no-dev", "python", "-m", "board_controls"],
        "metadata": {"area": "governance"},
        "types": [CONTROL_TYPE],
        "capabilities": [
            {
                "name": "for-path",
                "description": "The controls that apply to a path.",
                "inputSchema": ForPathInput.model_json_schema(),
                "outputSchema": ForPathOutput.model_json_schema(),
                "annotations": {"readOnlyHint": True},
                "_meta": {},
            },
            {
                "name": "check",
                "description": "Check every budget control. Fails naming each control over its budget or degraded.",
                "inputSchema": CheckInput.model_json_schema(),
                "outputSchema": CheckOutput.model_json_schema(),
                "annotations": {"readOnlyHint": True},
                "_meta": {},
            },
        ],
    }


def schema() -> dict[str, JsonValue]:
    """The schema of `controls/control/v1`, as the plugin ships it."""
    return ControlRecord.model_json_schema(by_alias=True)


if __name__ == "__main__":
    print(dumped(manifest()), end="")  # noqa: T201 - printing the manifest is this module's job
