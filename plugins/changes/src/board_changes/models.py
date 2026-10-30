"""The change record and the capabilities' inputs and outputs, from which the shipped schemas are generated.

A call reads them as plain JSON, which Board has validated against these
schemas, so a call loads no pydantic (STANDARDS.md P3).
"""

from collections.abc import Callable
from typing import Annotated, Literal, cast

from board_plugin.model import Strict
from pydantic import BaseModel, ConfigDict, Field
from pydantic.config import JsonDict
from pydantic.json_schema import SkipJsonSchema

from board_changes.change import CHANGE_TYPE
from board_changes.open import OPEN_LIMIT

# The most changes `open` returns.
OPEN_MAX = 10_000

Phase = Literal["plan", "build", "review", "land", "done", "dropped"]
Route = Literal["owner", "ask-first", "unowned"]
# References Board resolves and checks. Each is a plain property, never an `anyOf` with null.
Ref = Annotated[str, Field(json_schema_extra={"format": "board-ref"})]
Commit = Annotated[str, Field(pattern="^[0-9a-f]{7,40}$")]
Url = Annotated[str, Field(pattern="^https?://")]
# Input properties `board call` takes as flags, relative to the run's prefix.
LogicalPath = Annotated[str, Field(pattern="^/", json_schema_extra={"format": "board-path"})]

# `attach` changes at least one thing. pydantic cannot emit this itself.
ATTACH_SOMETHING: JsonDict = {
    "anyOf": [{"required": [name]} for name in ("ref", "route", "head", "issue", "pull_request")],
}


def shipped(extra: JsonDict | None = None) -> Callable[[JsonDict], None]:
    """A schema hook: no `null` default for a field typed otherwise, and `extra` keywords."""

    def hook(schema: JsonDict) -> None:
        for field in cast("dict[str, JsonDict]", schema.get("properties", {})).values():
            field.pop("default", None)
        schema.update(extra or {})

    return hook


class ChangeRecord(BaseModel):
    """`changes/change/v1`: one change to a repository, its phase, and every record it produced.

    A change lives at `/changes/<name>.json`; its plan and any files it owns
    sit in `/changes/<name>/`. Git history holds its earlier phases. `mount`
    references the repository's mount, such as `/repos/board`. `phase` moves
    from `plan` through `build`, `review` and `land` to `done`; `dropped`
    ends a change early. `route` is `owner` when no one else owns the files,
    `ask-first` when someone else does, and `unowned` when the repository
    names no owners. `plan` references the Markdown plan.
    `issue` links the spec issue, when there is one. `head` is the commit
    under test and review. `pull_request` is the pull request's URL.
    `records` references every record the change produced: tasks, runs,
    evals, questions, decisions and review reports.
    """

    model_config = ConfigDict(extra="forbid", frozen=True, json_schema_extra=shipped())

    tag: Literal["changes/change/v1"] = Field(alias="$type")
    title: str = Field(min_length=1)
    mount: Ref
    phase: Phase
    route: Route | SkipJsonSchema[None] = None
    plan: Ref | SkipJsonSchema[None] = None
    issue: Url | SkipJsonSchema[None] = None
    head: Commit | SkipJsonSchema[None] = None
    pull_request: Url | SkipJsonSchema[None] = None
    records: list[Ref] | SkipJsonSchema[None] = None


class StartInput(Strict):
    """Create a change in `plan` for a mounted repository."""

    model_config = ConfigDict(json_schema_extra=shipped())

    mount: LogicalPath = Field(description="The repository's mount, such as /repos/board.")
    title: str = Field(pattern="[A-Za-z0-9]", description="What the change does, in a few words.")
    name: str | SkipJsonSchema[None] = Field(
        default=None,
        pattern="^[a-z0-9][a-z0-9-]{0,59}$",
        description="The change's name, so it lives at /changes/<name>; made from the title when left out.",
    )
    handle: str | SkipJsonSchema[None] = Field(
        default=None,
        pattern=r"^@\S+$",
        description="Whose change this is, as CODEOWNERS names people, such as @luke; sets the route.",
    )


class PathInput(Strict):
    """One change, by path."""

    path: LogicalPath = Field(description="The change's logical path, such as /changes/suite-timeout.")


class AttachInput(Strict):
    """Add a record reference to a change, or set its route, head commit, issue or pull request."""

    model_config = ConfigDict(json_schema_extra=shipped(ATTACH_SOMETHING))

    path: LogicalPath = Field(description="The change's logical path, such as /changes/suite-timeout.")
    ref: LogicalPath | SkipJsonSchema[None] = Field(
        default=None, description="A record the change produced, such as /suites/board-tests/runs/<id>."
    )
    route: Route | SkipJsonSchema[None] = Field(default=None, description="The route: owner, ask-first or unowned.")
    head: Commit | SkipJsonSchema[None] = Field(default=None, description="The commit under test and review.")
    issue: Url | SkipJsonSchema[None] = Field(default=None, description="The URL of the issue the change answers.")
    pull_request: Url | SkipJsonSchema[None] = Field(default=None, description="The pull request's URL.")


class OpenInput(Strict):
    """The changes that are not done or dropped, by path."""

    limit: int = Field(
        default=OPEN_LIMIT,
        ge=1,
        le=OPEN_MAX,
        description=f"The most changes to return, 1 to {OPEN_MAX:,}; {OPEN_LIMIT} when left out.",
    )


class Rule(Strict):
    """One CODEOWNERS line: a pattern and the owners of the files it matches."""

    pattern: str
    owners: list[str]


class StartOutput(Strict):
    """The new change and the repository's brief.

    `owners` lists the CODEOWNERS rules from `codeowners`, the file read, and
    `ask_first` the patterns someone other than `handle` owns. `docs` lists
    the contributing docs found, to read before designing. `suite` is the
    check suite they name as a `runs/suite/v1` path, or `null`.
    """

    path: str
    plan: str
    route: Route
    owners: list[Rule]
    ask_first: list[str]
    codeowners: str | None
    docs: list[str]
    suite: str | None


class Check(Strict):
    """One requirement of the phase's gate: `pass`, or `missing` with what is missing in `detail`."""

    check: str
    status: Literal["pass", "missing"]
    detail: str


class NextOutput(Strict):
    """Where the change stands: its phase, route, the gate's checks and what to do now."""

    path: str
    phase: Phase
    route: Route | None
    gate: list[Check]
    instruction: str


class AttachOutput(Strict):
    """The change written, and how many records it now references."""

    path: str
    records: int


class AdvanceOutput(Strict):
    """The phase the change left and the phase it entered."""

    path: str
    from_: Phase = Field(alias="from")
    to: Phase


class OpenChange(Strict):
    """A change that is not done or dropped."""

    path: str
    title: str
    mount: str
    phase: Phase
    route: Route | None


class OpenOutput(Strict):
    """Open changes by path.

    `total` counts every open change and `truncated` says whether the limit
    left some out. `unreadable` is how many were left out because Board marks
    them degraded.
    """

    changes: list[OpenChange]
    total: int
    truncated: bool
    unreadable: int


class PrOutput(Strict):
    """The pull request drafted for a change.

    `path` is the logical path of `pr.md`, which holds the body. `command` is
    the `gh pr create` line, for a POSIX shell, that would open the pull
    request with that body; the plugin never runs it. When the change already
    records a pull request, `command` is `null`, `pull_request` holds its
    URL and `pr.md` is not written. `unreadable` lists the references that reach nothing or that Board
    marks degraded.
    """

    path: str
    command: str | None
    pull_request: str | None
    unreadable: list[str]


CHANGE_SCHEMA_PATH = f"{CHANGE_TYPE}.schema.json"
