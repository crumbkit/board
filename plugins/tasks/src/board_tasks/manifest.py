"""The plugin's manifest, generated from its models (STANDARDS.md P3)."""

import json

from pydantic import JsonValue

from board_tasks.models import DueInput, DueOutput


def manifest() -> dict[str, JsonValue]:
    """`board/plugin-manifest/v1` for this plugin."""
    return {
        "$type": "board/plugin-manifest/v1",
        "name": "tasks",
        "version": "0.1.0",
        "description": "Open tasks and their due dates.",
        "run": ["uv", "run", "--frozen", "--no-dev", "python", "-m", "board_tasks"],
        "metadata": {"area": "planning"},
        "extract": ["lr/task/v1"],
        "capabilities": [
            {
                "name": "due",
                "description": "Open tasks due before a date.",
                "input_schema": DueInput.model_json_schema(),
                "output_schema": DueOutput.model_json_schema(),
                "metadata": {},
            },
        ],
    }


def text() -> str:
    """The manifest as `plugin.json` holds it: sorted keys, two-space indents."""
    return json.dumps(manifest(), indent=2, sort_keys=True) + "\n"


if __name__ == "__main__":
    print(text(), end="")  # noqa: T201 - printing the manifest is this module's job
