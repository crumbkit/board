"""The change's records and the capabilities' inputs and outputs, from which the shipped schemas are generated.

A call reads them as plain JSON, which Board has validated against these
schemas, so a call loads no pydantic (STANDARDS.md P3).
"""

from typing import Annotated, Any, Literal, cast

from board_plugin.model import Strict
from pydantic import BaseModel, ConfigDict, Field
from pydantic.config import JsonDict
from pydantic.json_schema import SkipJsonSchema

from board_changes.change import (
    CHANGE_TYPE,
    CHECK_TYPE,
    CLAIM_TYPE,
    DROPPED_TYPE,
    FILES_TYPE,
    LANDED_TYPE,
    OPENED_TYPE,
    PLAN_TYPE,
    SUITE_TYPE,
    WORKFLOW_TYPE,
)
from board_changes.open import OPEN_LIMIT
from board_changes.plan import PLAN_WORDS, USES_WORDS

# The most changes `open` returns.
OPEN_MAX = 10_000

Route = Literal["owner", "ask-first", "unowned", "unread"]
Actor = Literal["agent", "reviewer", "owner"]
Kind = Literal["plan", "answer", "check", "review", "landed"]
Commit = Annotated[str, Field(pattern="^[0-9a-f]{7,64}$")]
Url = Annotated[str, Field(pattern="^https?://")]
# A name of lowercase letters, digits and hyphens: a step, a change or a repo record.
Slug = Annotated[str, Field(pattern="^[a-z0-9][a-z0-9-]{0,59}$")]
# A handle as CODEOWNERS names people, such as @luke.
Handle = Annotated[str, Field(pattern=r"^@\S+$")]
Session = Annotated[str, Field(pattern=r"\S")]
Reference = Annotated[str, Field(json_schema_extra={"format": "board-ref"})]
# Input properties `board call` takes as flags, relative to the run's prefix.
LogicalPath = Annotated[str, Field(pattern="^/", json_schema_extra={"format": "board-path"})]


def ref(description: str, record_type: str | None = None) -> Any:  # noqa: ANN401 - a pydantic Field, as Field returns
    """A required reference Board resolves and checks, to `record_type` or to any entry."""
    extra: JsonDict = {"format": "board-ref"}
    if record_type is not None:
        extra["x-board-type"] = record_type
    return Field(description=description, json_schema_extra=extra)


def untitled(schema: JsonDict) -> None:
    """A schema hook: no `title` and no `null` default on a property, as the shape rules ask."""
    for field in cast("dict[str, JsonDict]", schema.get("properties", {})).values():
        field.pop("title", None)
        field.pop("default", None)


def shipped(schema: JsonDict) -> None:
    """A schema hook: no `null` default for an input field typed otherwise."""
    for field in cast("dict[str, JsonDict]", schema.get("properties", {})).values():
        field.pop("default", None)


class Record(BaseModel):
    """A record the plugin ships the schema of: closed, and shaped as docs/architecture/records/shapes.md asks."""

    model_config = ConfigDict(extra="forbid", frozen=True, json_schema_extra=untitled)


class ChangeRecord(Record):
    """`changes/change/v2`: one change to a repository, at `/changes/<name>.json`.

    Its other records sit in `/changes/<name>/`, each written once, and its
    state is derived from which of them exist.
    """

    tag: Literal["changes/change/v2"] = Field(alias="$type", description="The record's type.")
    title: str = Field(min_length=1, description="What the change does, in a few words.")
    repo: str = ref("The repo record of the repository the change is to, such as /repos/board.")
    branch: str = Field(min_length=1, description="The work branch in the repository.")
    issue: Url | SkipJsonSchema[None] = Field(default=None, description="The URL of the issue the change answers.")
    workflow: str = ref("The workflow whose steps the change follows.", WORKFLOW_TYPE)
    suite: str = ref("The check suite whose runs the check step reads.", SUITE_TYPE)
    route: Route = Field(
        description="owner when nobody else owns a CODEOWNERS pattern, ask-first when someone does, unowned when no"
        " pattern names anyone, and unread when the board holds no checkout to read CODEOWNERS from."
    )


class Step(Record):
    """One step of a workflow: who takes it and the kind of record that completes it."""

    name: Slug = Field(description="The step's name, such as review.")
    actor: Actor = Field(description="Who takes the step: agent, reviewer or owner.")
    done_by: Kind = Field(description="The kind of record that completes the step; README.md gives each kind's rule.")


class WorkflowRecord(Record):
    """`changes/workflow/v1`: the steps a change follows, in order, at `/workflows/<name>`."""

    tag: Literal["changes/workflow/v1"] = Field(alias="$type", description="The record's type.")
    steps: list[Step] = Field(min_length=1, description="The steps, in order.")


class ClaimRecord(Record):
    """`changes/claim/v1`, at `/changes/<name>/claims/<stamp>`: who holds the change and where the work is."""

    tag: Literal["changes/claim/v1"] = Field(alias="$type", description="The record's type.")
    session: Session = Field(description="The session that holds the change.")
    worktree: str = Field(pattern="^/", description="The root of the clone the session works in.")


class CheckRecord(Record):
    """`changes/check/v1`, at `/changes/<name>/checks/<stamp>`: a run of the check suite at a head."""

    tag: Literal["changes/check/v1"] = Field(alias="$type", description="The record's type.")
    run: str = ref("The run, which names the head commit in its versions.", "runs/run/v1")


class FilesRecord(Record):
    """`changes/files/v1`, at `/changes/<name>/files/<head>`: the paths the branch touches at that head."""

    tag: Literal["changes/files/v1"] = Field(alias="$type", description="The record's type.")
    files: list[str] = Field(description="Paths relative to the repository's root, as git diff --name-only gives.")


class OpenedRecord(Record):
    """`changes/opened/v1`, at `/changes/<name>/opened`: the change's pull request."""

    tag: Literal["changes/opened/v1"] = Field(alias="$type", description="The record's type.")
    pull_request: Url = Field(description="The pull request's URL.")


class LandedRecord(Record):
    """`changes/landed/v1`, at `/changes/<name>/landed`: the change merged, which ends it."""

    tag: Literal["changes/landed/v1"] = Field(alias="$type", description="The record's type.")
    commit: Commit = Field(description="The merge commit.")


class DroppedRecord(Record):
    """`changes/dropped/v1`, at `/changes/<name>/dropped`: the change stopped, which ends it."""

    tag: Literal["changes/dropped/v1"] = Field(alias="$type", description="The record's type.")
    why: str = Field(pattern=r"\S", description="Why the change stopped.")


class PlanRecord(Record):
    """`changes/plan/v1`: one plan for a change, and the records it uses.

    A plan lives at `/changes/<name>/plans/<stamp>.json`, beside its text at
    `<stamp>.md`; the newest stamp is the current plan. `text` references the
    Markdown. `uses` references the standards documents, controls, decisions,
    evals and other changes the plan relies on.
    """

    tag: Literal["changes/plan/v1"] = Field(alias="$type", description="The record's type.")
    text: str = ref("The plan's Markdown text, beside this record.")
    uses: list[Reference] = Field(description="The records and documents the plan relies on.")


class PathInput(Strict):
    """One change, by path."""

    path: LogicalPath = Field(description="The change's logical path, such as /changes/suite-timeout.")


class PlanInput(Strict):
    """A plan for a change, given on stdin because `uses` is an array."""

    path: LogicalPath = Field(description="The change's logical path, such as /changes/suite-timeout.")
    text: str = Field(pattern=r"\S", description=f"The plan in Markdown, at most {PLAN_WORDS:,} words.")
    uses: list[LogicalPath] = Field(
        description="The records and documents the plan relies on; the documents hold at most "
        f"{USES_WORDS:,} words in all."
    )


class PlanOutput(Strict):
    """The plan record written, its text, and how many references it uses."""

    path: str
    text: str
    uses: int


class StartInput(Strict):
    """Start a change to the repository of the clone Board was started in."""

    model_config = ConfigDict(json_schema_extra=shipped)

    title: str = Field(pattern="[A-Za-z0-9]", description="What the change does, in a few words.")
    session: Session = Field(description="The session starting the change, which its first claim names.")
    name: Slug | SkipJsonSchema[None] = Field(
        default=None,
        description="The change's name, so it lives at /changes/<name>; made from the title when left out.",
    )
    handle: Handle | SkipJsonSchema[None] = Field(
        default=None,
        description="Whose change this is, as CODEOWNERS names people, such as @luke; sets the route.",
    )
    issue: Url | SkipJsonSchema[None] = Field(default=None, description="The URL of the issue the change answers.")
    suite: LogicalPath | SkipJsonSchema[None] = Field(
        default=None, description="The check suite, a runs/suite/v1; read from the contributing docs when left out."
    )
    workflow: LogicalPath | SkipJsonSchema[None] = Field(
        default=None, description="The workflow, a changes/workflow/v1; chosen by the repository when left out."
    )
    repo_name: Slug | SkipJsonSchema[None] = Field(
        default=None,
        alias="repo-name",
        description="The name of a new repo record, /repos/<name>; the remote's last segment when left out.",
    )


class Rule(Strict):
    """One CODEOWNERS line: a pattern and the owners of the files it matches."""

    pattern: str
    owners: list[str]


class StartOutput(Strict):
    """The new change, its repository and the repository's brief.

    `repo` is the repo record, and `repo_created` says whether `start` wrote
    it. `claim` is the first claim. `owners` lists the CODEOWNERS rules from
    `codeowners`, the file read, and `ask_first` the patterns someone other
    than `handle` owns. `docs` lists the contributing docs found, to read
    before designing.
    """

    path: str
    repo: str
    repo_created: bool
    branch: str
    workflow: str
    suite: str
    route: Route
    claim: str
    owners: list[Rule]
    ask_first: list[str]
    codeowners: str | None
    docs: list[str]


class SeedInput(Strict):
    """Nothing: `seed` writes the workflows it ships."""


class SeedOutput(Strict):
    """The workflows `seed` wrote; those already there are left as they are."""

    written: list[str]


class ClaimInput(Strict):
    """Claim a change for a session, from the clone it works in."""

    path: LogicalPath = Field(description="The change's logical path, such as /changes/suite-timeout.")
    session: Session = Field(description="The session that holds the change from now on.")


class Written(Strict):
    """The record written."""

    path: str


class ClaimOutput(Strict):
    """The claim written and the worktree it names."""

    path: str
    worktree: str


class FilesInput(Strict):
    """The paths the branch touches at a head, given on stdin because `files` is an array."""

    path: LogicalPath = Field(description="The change's logical path, such as /changes/suite-timeout.")
    head: Commit = Field(description="The commit the paths were read at.")
    files: list[str] = Field(description="Paths relative to the repository's root, as git diff --name-only gives.")


class CheckInput(Strict):
    """Record a run of the check suite at the head it names."""

    path: LogicalPath = Field(description="The change's logical path, such as /changes/suite-timeout.")
    run: LogicalPath = Field(description="The runs/run/v1 the suite wrote, such as /suites/<name>/runs/<id>.")


class CheckOutput(Strict):
    """The check written and the head its run names."""

    path: str
    head: str


class LandedInput(Strict):
    """End the change as merged."""

    path: LogicalPath = Field(description="The change's logical path, such as /changes/suite-timeout.")
    commit: Commit = Field(description="The merge commit.")


class DropInput(Strict):
    """End the change as stopped."""

    path: LogicalPath = Field(description="The change's logical path, such as /changes/suite-timeout.")
    why: str = Field(pattern=r"\S", description="Why the change stopped.")


class StepState(Strict):
    """One step of the change: whether it is done and by which record.

    `added_by` is the control that added the step, `route` for the approve
    step a route adds, or null for a step of the workflow. `if_touched` says
    a control adds the step only if the branch touches its files, which no
    files record at the head says yet; record the files with `files` to
    settle it.
    """

    name: str
    actor: Actor
    done_by: Kind
    done: bool
    record: str | None
    added_by: str | None
    if_touched: bool


class NextOutput(Strict):
    """Where the change stands: its head, its steps, the next step and its actor, and how it ended."""

    path: str
    head: str | None
    steps: list[StepState]
    next: str | None
    actor: Actor | None
    ended: Literal["landed", "dropped"] | None


class BriefUse(Strict):
    """One record the plan uses, and one line saying what it holds."""

    ref: str
    line: str


class BriefOutput(Strict):
    """What an agent needs to start a change, in at most 600 words.

    `plan` is the newest plan's text, or `null` when the change has no plan
    record. `uses` gives one line per record it uses, in order, and
    `left_out` counts those the word limit left out. `controls` is the
    command that lists the controls on the caller's repository.
    """

    path: str
    plan: str | None
    uses: list[BriefUse]
    left_out: int
    next: str | None
    actor: Actor | None
    controls: str


class OpenInput(Strict):
    """The changes that have not ended, by path."""

    limit: int = Field(
        default=OPEN_LIMIT,
        ge=1,
        le=OPEN_MAX,
        description=f"The most changes to return, 1 to {OPEN_MAX:,}; {OPEN_LIMIT} when left out.",
    )


class OpenChange(Strict):
    """A change that has not ended."""

    path: str
    title: str
    repo: str
    branch: str
    route: Route


class OpenOutput(Strict):
    """Changes with no landed or dropped record, by path.

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
    request with that body; the plugin never runs it. When the change has an
    `opened` record, `command` is `null`, `pull_request` holds its URL and
    `pr.md` is not written. `unreadable` lists the references that reach
    nothing or that Board marks degraded.
    """

    path: str
    command: str | None
    pull_request: str | None
    unreadable: list[str]


class Given(Strict):
    """What the owner gives for one v1 change whose record cannot supply it."""

    model_config = ConfigDict(json_schema_extra=shipped)

    branch: str | SkipJsonSchema[None] = Field(default=None, description="The change's branch.")
    suite: Reference | SkipJsonSchema[None] = Field(
        default=None, description="The change's check suite, a runs/suite/v1."
    )
    commit: Commit | SkipJsonSchema[None] = Field(
        default=None, description="The merge commit of a change whose v1 phase is done."
    )


class MigrateInput(Strict):
    """Whether to write, and what the owner gives for v1 changes, by path."""

    model_config = ConfigDict(json_schema_extra=shipped)

    apply: bool = Field(default=False, description="Write the lines; without it, only list them.")
    changes: dict[str, Given] = Field(
        default_factory=dict, description="The branch or suite of each v1 change, by its logical path."
    )


class MigrateOutput(Strict):
    """The lines `migrate` wrote or would write, and the v1 changes it left, in `owed`."""

    applied: bool
    commit: str | None
    lines: list[dict[str, Any]]
    total: int
    truncated: bool
    owed: list[str]


# Each record type the plugin ships, by its type.
RECORDS: dict[str, type[BaseModel]] = {
    CHANGE_TYPE: ChangeRecord,
    PLAN_TYPE: PlanRecord,
    WORKFLOW_TYPE: WorkflowRecord,
    CLAIM_TYPE: ClaimRecord,
    CHECK_TYPE: CheckRecord,
    FILES_TYPE: FilesRecord,
    OPENED_TYPE: OpenedRecord,
    LANDED_TYPE: LandedRecord,
    DROPPED_TYPE: DroppedRecord,
}
