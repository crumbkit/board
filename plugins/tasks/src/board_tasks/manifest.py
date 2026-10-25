"""The plugin's manifest and the schema it ships, generated from its models (STANDARDS.md P3)."""

from board_plugin.protocol import dumped
from pydantic import JsonValue

from board_tasks.models import (
    TASK_TYPE,
    TASK_V2_TYPE,
    AddInput,
    AvailableInput,
    AvailableOutput,
    DeferInput,
    DueInput,
    DueOutput,
    NextInput,
    NextOutput,
    TaskPath,
    TaskRecord,
    TaskV2,
    Written,
)


def manifest() -> dict[str, JsonValue]:
    """`board/plugin-manifest/v1` for this plugin."""
    return {
        "$type": "board/plugin-manifest/v1",
        "name": "tasks",
        "version": "0.1.0",
        # SKILL.md's description, the callout an agent sees first.
        "description": (
            "Sweep the person's tasks one at a time, marking each done, deferring, dropping or editing it, and add a "
            "task. Use it when the person asks to sweep, review or go through their tasks or to-do list, or to add a "
            "task to their own list. Start with `board -C <board> call tasks.available`, run in the repository you "
            "work in, which syncs the notes repos that repository's git remote routes to and lists the tasks to sweep. "
            "SKILL.md and README.md in the plugin's directory say more."
        ),
        "run": ["uv", "run", "--frozen", "--no-dev", "python", "-m", "board_tasks"],
        "metadata": {"area": "planning"},
        "extract": [TASK_TYPE, TASK_V2_TYPE],
        "types": [TASK_TYPE, TASK_V2_TYPE],
        "capabilities": [
            {
                "name": "available",
                "description": (
                    "Sync the repos the caller's remote chooses, then list their open tasks available now, those due"
                    " within seven days, and the stale ones."
                ),
                "inputSchema": AvailableInput.model_json_schema(),
                "outputSchema": AvailableOutput.model_json_schema(),
                "_meta": {},
            },
            *writes(),
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


def writes() -> list[dict[str, JsonValue]]:
    """The write calls, each of which syncs the task's repo, writes the task with `reviewed` today, and pushes."""
    calls = [
        ("add", AddInput, "Write a new open task to tasks/<slug>.md in the repo the caller's remote routes to."),
        ("done", TaskPath, "Mark a task done, or defer a repeating one to its next occurrence."),
        ("defer", DeferInput, "Defer a task until a date or a horizon from today, written as a date."),
        ("drop", TaskPath, "Mark a task dropped."),
        ("reviewed", TaskPath, "Mark a task reviewed today."),
    ]
    return [
        {
            "name": name,
            "description": description + " It syncs the task's repo first and pushes after, and sets reviewed.",
            "inputSchema": model.model_json_schema(),
            "outputSchema": Written.model_json_schema(),
            "_meta": {},
        }
        for name, model, description in calls
    ]


def schema() -> dict[str, JsonValue]:
    """The schema of `tasks/task/v1`, as the plugin ships it."""
    return TaskRecord.model_json_schema(by_alias=True)


def schema_v2() -> dict[str, JsonValue]:
    """The schema of `tasks/task/v2`, as the plugin ships it."""
    return TaskV2.model_json_schema(by_alias=True)


def text() -> str:
    """The manifest as `plugin.json` holds it: sorted keys, two-space indents."""
    return dumped(manifest())


if __name__ == "__main__":
    print(text(), end="")  # noqa: T201 - printing the manifest is this module's job
