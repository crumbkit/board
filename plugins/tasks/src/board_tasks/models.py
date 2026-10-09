"""The task record, and the plugin's inputs and outputs.

The record's schema, which the plugin ships, and the manifest's schemas are
generated from these models.
"""

import datetime
from typing import Annotated, Literal, cast

from board_plugin.protocol import Strict
from pydantic import BaseModel, ConfigDict, Field
from pydantic.config import JsonDict
from pydantic.json_schema import SkipJsonSchema

TASK_TYPE = "lr/task/v1"

# The statuses of a task that is not finished yet; `done` and `dropped` are finished.
UNFINISHED = ("open", "waiting")

# The most tasks `next` lists. It runs one `board refs` for each listed task
# with an `after`, and a call has 30 seconds. Each takes about 50 ms on an idle
# Mac, and 100 such tasks took 12 seconds on a heavily loaded one. Batching
# those lookups into one `board refs` is the way to raise it.
NEXT_LIMIT = 100

# References Board resolves and checks. Each is a plain property, never an
# `anyOf` with null, because Board does not search `anyOf` for references.
NotesRef = Annotated[str, Field(json_schema_extra={"format": "board-ref"})]
TaskRef = Annotated[str, Field(json_schema_extra={"format": "board-ref", "x-board-type": TASK_TYPE})]


def shipped(schema: JsonDict) -> None:
    """The schema as the plugin ships it: no other field, and no `null` default for a field typed string."""
    schema["additionalProperties"] = False
    for field in cast("dict[str, JsonDict]", schema["properties"]).values():
        field.pop("default", None)


class TaskRecord(BaseModel):
    """`lr/task/v1`: something to do, its status, and optionally when it is due and which tasks it waits on.

    `notes` references a Markdown file holding the task's prose, such as its
    phase, pull request, plan and review. `after` references the tasks this
    one waits on. A project is a directory, `/projects/<name>/`, so a task
    names none, and git history holds a task's earlier states.
    """

    # The schema refuses any other field, but a board may keep a looser schema
    # of its own (STANDARDS.md 3), so the plugin reads a task ignoring the rest.
    model_config = ConfigDict(extra="ignore", frozen=True, json_schema_extra=shipped)

    tag: Literal["lr/task/v1"] = Field(alias="$type")
    title: str
    status: Literal["open", "waiting", "done", "dropped"]
    due: datetime.date | SkipJsonSchema[None] = None
    notes: NotesRef | SkipJsonSchema[None] = None
    after: list[TaskRef] = Field(default_factory=list[str])


class DueInput(Strict):
    """Unfinished tasks due before this date."""

    before: datetime.date


class DueTask(Strict):
    """An unfinished task and when it is due."""

    path: str
    title: str
    due: datetime.date


class DueOutput(Strict):
    """Unfinished tasks, `open` or `waiting`, due before the date asked for, by due date then path."""

    tasks: list[DueTask]
    truncated: bool


class NextInput(Strict):
    """The unfinished tasks to work on next, at most `limit` of them."""

    limit: int = Field(default=NEXT_LIMIT, ge=1, le=NEXT_LIMIT)


class NextTask(Strict):
    """An unfinished task. It is blocked while a task in its `after` is not `done`; `blocked_by` names each."""

    path: str
    title: str
    status: Literal["open", "waiting"]
    due: datetime.date | None
    blocked: bool
    blocked_by: list[str]


class NextOutput(Strict):
    """Unfinished tasks, `open` or `waiting`, by due date with undated ones last, then by path.

    `total` counts every unfinished task, and `truncated` says whether some
    were left out to keep within the limit.
    """

    tasks: list[NextTask]
    total: int
    truncated: bool
