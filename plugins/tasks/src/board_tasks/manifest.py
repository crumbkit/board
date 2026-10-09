"""The plugin's manifest and the schema it ships, generated from its models (STANDARDS.md P3)."""

from board_plugin.protocol import dumped
from pydantic import JsonValue

from board_tasks.models import TASK_TYPE, DueInput, DueOutput, NextInput, NextOutput, TaskRecord


def manifest() -> dict[str, JsonValue]:
    """`board/plugin-manifest/v1` for this plugin."""
    return {
        "$type": "board/plugin-manifest/v1",
        "name": "tasks",
        "version": "0.1.0",
        "description": "Tasks, their due dates and what each waits on.",
        "run": ["uv", "run", "--frozen", "--no-dev", "python", "-m", "board_tasks"],
        "metadata": {"area": "planning"},
        "extract": [TASK_TYPE],
        "types": [TASK_TYPE],
        "capabilities": [
            {
                "name": "due",
                "description": "Unfinished tasks, open or waiting, due before a date.",
                "inputSchema": DueInput.model_json_schema(),
                "outputSchema": DueOutput.model_json_schema(),
                "_meta": {},
            },
            {
                "name": "next",
                "description": (
                    "Unfinished tasks by due date then path, each blocked while a task it waits on is not done."
                ),
                "inputSchema": NextInput.model_json_schema(),
                "outputSchema": NextOutput.model_json_schema(),
                "annotations": {"readOnlyHint": True},
                "_meta": {},
            },
        ],
    }


def schema() -> dict[str, JsonValue]:
    """The schema of `tasks/task/v1`, as the plugin ships it."""
    return TaskRecord.model_json_schema(by_alias=True)


def text() -> str:
    """The manifest as `plugin.json` holds it: sorted keys, two-space indents."""
    return dumped(manifest())


if __name__ == "__main__":
    print(text(), end="")  # noqa: T201 - printing the manifest is this module's job
