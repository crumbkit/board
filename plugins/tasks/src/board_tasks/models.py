"""The task record, and the plugin's inputs and outputs.

The record's schema, which the plugin ships, and the manifest's schemas are
generated from these models.
"""

import datetime
from typing import Annotated, Any, Literal, cast

from board_plugin.model import Strict
from pydantic import BaseModel, ConfigDict, Field
from pydantic.config import JsonDict
from pydantic.json_schema import SkipJsonSchema

TASK_TYPE = "tasks/task/v1"
TASK_V2_TYPE = "tasks/task/v2"

# The statuses of a task that is not finished yet; `done` and `dropped` are finished.
UNFINISHED = ("open", "waiting")

# Codes that depend on the tree rather than the record. A task holding one,
# such as an `after` that names no task yet, is still a task to list, and,
# when it is done, still done.
TREE_CODES = ("board::ref::dangling", "board::ref::wrong_type")

# What `board refs` reads from each task a task's `after` reaches, to tell whether it is done.
FIELDS = ("/status", "/$type")

# The most tasks `next` lists, and its default `limit`. The cap keeps the
# output bounded (STANDARDS 23).
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
    """`tasks/task/v1`: something to do, its status, and optionally when it is due and which tasks it waits on.

    `notes` references a Markdown file holding the task's prose, such as its
    phase, pull request, plan and review. `after` references the tasks this
    one waits on. A project is a directory, `/projects/<name>/`, so a task
    names none, and git history holds a task's earlier states.
    """

    # The schema refuses any other field, but a board may keep a looser schema
    # of its own (STANDARDS.md 3), so the plugin reads a task ignoring the rest.
    model_config = ConfigDict(extra="ignore", frozen=True, json_schema_extra=shipped)

    tag: Literal["tasks/task/v1"] = Field(alias="$type")
    title: str
    status: Literal["open", "waiting", "done", "dropped"]
    due: datetime.date | SkipJsonSchema[None] = None
    notes: NotesRef | SkipJsonSchema[None] = None
    after: list[TaskRef] = Field(default_factory=list[str])


class DueInput(Strict):
    """Unfinished tasks due before this date."""

    before: datetime.date = Field(description="The date, as YYYY-MM-DD, that the listed tasks are due before.")


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

    limit: int = Field(
        default=NEXT_LIMIT,
        ge=1,
        le=NEXT_LIMIT,
        description=f"The most tasks to list, 1 to {NEXT_LIMIT}; {NEXT_LIMIT} when left out.",
    )


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


# `daily`, `weekdays`, `weekly-<day>`, `monthly-<1..31>` and `annually-<MM-DD>`.
REPEAT = (
    r"^(daily|weekdays|weekly-(mon|tue|wed|thu|fri|sat|sun)|monthly-([1-9]|[12][0-9]|3[01])"
    r"|annually-(0[1-9]|1[0-2])-(0[1-9]|[12][0-9]|3[01]))$"
)
TaskV2Ref = Annotated[str, Field(json_schema_extra={"format": "board-ref", "x-board-type": TASK_V2_TYPE})]


def shipped_v2(schema: JsonDict) -> None:
    """As `shipped`, with `ephemeral` only beside `repeat`, and admitting other fields.

    Reflect writes fields of its own, such as `id` and `pinned`, and a task must not degrade for them.
    """
    shipped(schema)
    del schema["additionalProperties"]
    schema["dependentRequired"] = {"ephemeral": ["repeat"]}


class TaskV2(BaseModel):
    """`tasks/task/v2`: the frontmatter of a Markdown task under `tasks/`, whose body holds its prose.

    `defer` hides the task until a date, a date-time or a horizon after `reviewed`.
    `after` references the tasks in the same repo that it waits on. `repeat`
    names when a repeating task comes back, and `ephemeral` that a missed day
    is not kept. `from_hour` is the hour of the person's day it is available
    from. `contexts` tags where it can be done. `reviewed` is the day it was
    last looked at. Any other field, such as Reflect's `id` and `pinned`, is
    admitted and ignored.
    """

    model_config = ConfigDict(extra="ignore", frozen=True, json_schema_extra=shipped_v2)

    tag: Literal["tasks/task/v2"] = Field(alias="$type")
    title: str
    status: Literal["open", "done", "dropped"]
    due: datetime.date | SkipJsonSchema[None] = None
    defer: (
        datetime.date
        | Annotated[datetime.datetime, Field(json_schema_extra={"format": "date-time"})]
        | Literal["not-today", "not-this-week", "soon", "someday"]
        | SkipJsonSchema[None]
    ) = None
    after: list[TaskV2Ref] = Field(default_factory=list[str])
    repeat: Annotated[str, Field(pattern=REPEAT)] | SkipJsonSchema[None] = None
    ephemeral: bool | SkipJsonSchema[None] = None
    from_hour: Annotated[int, Field(ge=0, le=23)] | SkipJsonSchema[None] = None
    contexts: list[Annotated[str, Field(pattern=r"^[a-z0-9-]+$")]] = Field(default_factory=list[str])
    reviewed: datetime.date | SkipJsonSchema[None] = None


def unset(schema: JsonDict) -> None:
    """Leave out the `null` default of a value the caller may leave out."""
    schema.pop("default", None)


SCOPE = {
    "repo": Field(
        default=None,
        json_schema_extra=unset,
        pattern="^/",
        description=(
            "One repo's tasks: / for the board's own tree, or a notes repo's mount path such as "
            "/repos/notes-personal; the caller's git remote chooses the repos when left out."
        ),
    ),
    "remote": Field(
        default=None,
        json_schema_extra=unset,
        min_length=1,
        description=(
            "A git remote, or owner/name, to choose the repos by in place of the origin of the directory board "
            "was started in."
        ),
    ),
}


class AvailableInput(Strict):
    """The tasks to sweep now, in the repos the caller's remote chooses."""

    context: str | SkipJsonSchema[None] = Field(
        default=None,
        json_schema_extra=unset,
        pattern=r"^[a-z0-9-]+$",
        description="Only tasks tagged with this context, such as errands.",
    )
    repo: str | SkipJsonSchema[None] = SCOPE["repo"]
    remote: str | SkipJsonSchema[None] = SCOPE["remote"]


class SweptTask(Strict):
    """An open task: where it is, why it waits, and how many open tasks wait on it."""

    repo: str
    path: str
    title: str
    due: datetime.date | None
    defer: str | None
    reviewed: datetime.date | None
    contexts: list[str]
    blocked_by: list[str]
    waiting: int


class Section(Strict):
    """At most 100 tasks of a section, by repo then title, and how many it holds."""

    tasks: list[SweptTask]
    total: int


class Unsynced(Strict):
    """A repo that did not sync, and the code why; its tasks are as its clone holds them."""

    repo: str
    code: str


class CrossRepo(Strict):
    """An `after` that names a task in another notes repo than its task's own, which is not followed."""

    path: str
    after: str


class AvailableOutput(Strict):
    """The sweep.

    `available` holds the open tasks not deferred, not blocked and past their
    `from_hour`. `due_soon` holds every open task due within seven days, or
    overdue. `stale` holds the open tasks not available or deferred that have
    no `reviewed`, or were reviewed over 14 days ago. `repos` are the repos
    read, `degraded` counts their Markdown files under `tasks/` that are not a
    readable open `tasks/task/v2` task, or hold a date that cannot be read,
    and `unsynced` lists the repos whose sync or pin failed. `cross_repo`
    lists the `after` entries that name another repo's task, which never
    block. `origin_unread` says why the caller's git repository gave no
    origin, when it did not, so only `/` and the personal repo were read.
    """

    available: Section
    due_soon: Section
    stale: Section
    repos: list[str]
    degraded: int
    unsynced: list[Unsynced]
    cross_repo: list[CrossRepo]
    origin_unread: str | None


# A string, so that `board call` offers it as a flag: a date or a horizon.
UNTIL = r"^([0-9]{4}-[0-9]{2}-[0-9]{2}|not-today|not-this-week|soon|someday)$"
UNTIL_HELP = (
    "A date, YYYY-MM-DD, or a horizon from today: not-today (1 day), not-this-week (6), soon (20) or someday (90). "
    "The task is written with the date."
)


class TaskPath(Strict):
    """The task to act on."""

    path: str = Field(
        pattern=r"^/.*\.md$",
        description="The task's path in the board, such as /tasks/call-the-bank.md or /repos/personal/tasks/x.md.",
    )


class DeferInput(TaskPath):
    """The task to defer, and until when."""

    until: str = Field(pattern=UNTIL, description=UNTIL_HELP)


def optional(description: str, **constraints: Any) -> Any:  # noqa: ANN401 - a pydantic Field, typed Any as Field is
    """The field of a value the caller may leave out, with its description and pydantic `constraints`."""
    return Field(default=None, json_schema_extra=unset, description=description, **constraints)


class AddInput(Strict):
    """A new open task, and the fields it starts with."""

    title: str = Field(min_length=1, description="The task's title, which also names its file.")
    text: str | SkipJsonSchema[None] = optional("The task's prose, written as its body.")
    due: str | SkipJsonSchema[None] = optional(
        "The date, YYYY-MM-DD, the task is due by.", pattern=r"^[0-9]{4}-[0-9]{2}-[0-9]{2}$"
    )
    defer: str | SkipJsonSchema[None] = optional(UNTIL_HELP, pattern=UNTIL)
    after: list[str] | SkipJsonSchema[None] = optional(
        "The paths, in the task's own repo, of the tasks it waits on, such as /tasks/agree-scope.md."
    )
    repeat: str | SkipJsonSchema[None] = optional(
        "When it comes back once done: daily, weekdays, weekly-mon, monthly-15 or annually-10-25.", pattern=REPEAT
    )
    ephemeral: bool | SkipJsonSchema[None] = optional("Whether a missed day of a repeating task is dropped.")
    from_hour: int | SkipJsonSchema[None] = optional(
        "The hour of the person's day, 0 to 23, the task is available from.", ge=0, le=23
    )
    contexts: list[Annotated[str, Field(pattern=r"^[a-z0-9-]+$")]] | SkipJsonSchema[None] = optional(
        "Tags of where it can be done, such as errands."
    )
    repo: str | SkipJsonSchema[None] = optional(
        "The repo to write the task to: / for the board's own tree, or a notes repo's mount path; the caller's git "
        "remote chooses it when left out.",
        pattern="^/",
    )
    remote: str | SkipJsonSchema[None] = SCOPE["remote"]


class Written(Strict):
    """The task as a write call left it.

    `commit` holds the write, None when the board's own tree left it uncommitted. `unsynced` is the code of
    a sync before the write that left the clone where it was, and `unpushed` the code of a push after it
    that failed; both are `tasks::repo::not_synced` for the board's own tree, which is never synced.
    """

    repo: str
    path: str
    status: str
    defer: str | None
    reviewed: str
    commit: str | None
    unsynced: str | None
    unpushed: str | None
