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

from board_changes.change import CHANGE_TYPE, PLAN_TYPE, PULL_TYPES, TASK_TYPE, TICKET_TYPES, UNDERSTANDING_TYPE
from board_changes.open import OPEN_LIMIT
from board_changes.plan import PLAN_WORDS, USES_WORDS

# The most changes `open` returns.
OPEN_MAX = 10_000

Phase = Literal["plan", "build", "review", "land", "done", "dropped"]
Route = Literal["owner", "ask-first", "unowned"]
Kind = Literal["build", "prototype"]
State = Literal["exploring", "settled", "abandoned"]
Ended = Literal["settled", "abandoned"]
# References Board resolves and checks. Each is a plain property, never an `anyOf` with null.
Ref = Annotated[str, Field(json_schema_extra={"format": "board-ref"})]
TicketRef = Annotated[str, Field(json_schema_extra={"format": "board-ref", "x-board-type": list(TICKET_TYPES)})]
PullRef = Annotated[str, Field(json_schema_extra={"format": "board-ref", "x-board-type": list(PULL_TYPES)})]
TaskRef = Annotated[str, Field(json_schema_extra={"format": "board-ref", "x-board-type": TASK_TYPE})]
# The shared shapes of crates/board-plugin/shapes.json (docs/architecture/records/shapes.md, rule 14).
Prose = Annotated[str, Field(min_length=1, json_schema_extra={"x-board-prose": True})]
SharedUrl = Annotated[
    str, Field(pattern="^https?://([^/?#@]*@)?[^/?#@:][^/?#@]*([/?#]|$)", json_schema_extra={"format": "uri"})
]
Commit = Annotated[str, Field(pattern="^[0-9a-f]{7,40}$")]
Url = Annotated[str, Field(pattern="^https?://")]
# Input properties `board call` takes as flags, relative to the run's prefix.
LogicalPath = Annotated[str, Field(pattern="^/", json_schema_extra={"format": "board-path"})]

# `attach` changes at least one thing. pydantic cannot emit this itself.
ATTACH_SOMETHING: JsonDict = {
    "anyOf": [
        {"required": [name]} for name in ("ref", "ticket", "pull", "task", "route", "head", "issue", "pull_request")
    ],
}


def shipped(extra: JsonDict | None = None) -> Callable[[JsonDict], None]:
    """A schema hook: no `null` default for a field typed otherwise, and `extra` keywords."""

    def hook(schema: JsonDict) -> None:
        for field in cast("dict[str, JsonDict]", schema.get("properties", {})).values():
            field.pop("default", None)
        schema.update(extra or {})

    return hook


def untitled(schema: JsonDict) -> None:
    """A schema hook: no `title` on a property, which the shape rules forbid (docs/architecture/records/shapes.md)."""
    for field in cast("dict[str, JsonDict]", schema.get("properties", {})).values():
        field.pop("title", None)


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
    `tickets` references the Jira or GitHub issues the change works on, and
    `pulls` its GitHub pull requests, each as a record of that type.
    `records` references every record the change produced: tasks, runs,
    evals, questions, decisions and review reports.
    `task` references the task the change does. `kind` is `build`, as when
    it is left out, for work that lands on main, or `prototype` for work that
    answers `question` on branch `proto/<name>` and never lands. A
    prototype's `state` is `exploring` until `verdict`, one sentence, settles
    or abandons it; `verdict_artifact` is the URL of the artifact version
    the verdict means.
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
    tickets: list[TicketRef] | SkipJsonSchema[None] = None
    pulls: list[PullRef] | SkipJsonSchema[None] = None
    records: list[Ref] | SkipJsonSchema[None] = None
    task: TaskRef | SkipJsonSchema[None] = None
    kind: Kind | SkipJsonSchema[None] = None
    question: Prose | SkipJsonSchema[None] = None
    state: State | SkipJsonSchema[None] = None
    verdict: Prose | SkipJsonSchema[None] = None
    verdict_artifact: SharedUrl | SkipJsonSchema[None] = None


class PlanRecord(BaseModel):
    """`changes/plan/v1`: one plan for a change, and the records it uses.

    A plan lives at `/changes/<name>/plans/<stamp>.json`, beside its text at
    `<stamp>.md`; the newest stamp is the current plan. `text` references the
    Markdown. `uses` references the standards documents, controls, decisions,
    evals and other changes the plan relies on.
    """

    model_config = ConfigDict(extra="forbid", frozen=True, json_schema_extra=untitled)

    tag: Literal["changes/plan/v1"] = Field(alias="$type", description="The record's type.")
    text: Ref = Field(description="The plan's Markdown text, beside this record.")
    uses: list[Ref] = Field(description="The records and documents the plan relies on.")


class Asked(BaseModel):
    """One question of an understanding check, the owner's answer, and whether it matches the plan."""

    model_config = ConfigDict(extra="forbid", frozen=True, json_schema_extra=untitled)

    round: int = Field(ge=1, description="The round it was asked in; a round holds at most three questions.")
    kind: Literal["predict", "choose", "explain", "edge"] = Field(description="The kind of question.")
    question: Prose = Field(description="The question, as asked.")
    answer: Prose = Field(description="The owner's answer, in his words.")
    matches: bool = Field(description="Whether the answer matches the plan.")


class Mismatch(BaseModel):
    """An answer that differed from the plan, and which of the two changed to settle it."""

    model_config = ConfigDict(extra="forbid", frozen=True, json_schema_extra=untitled)

    question: Prose = Field(description="The question whose answer differed.")
    difference: Prose = Field(description="How the answer and the plan differ, said plainly.")
    settled: Literal["plan", "expectation"] = Field(
        description="plan when the plan changed, expectation when the owner's expectation did."
    )
    how: Prose = Field(description="What changed.")


class Recheck(BaseModel):
    """A predict or edge question asked again, worded differently, before review."""

    model_config = ConfigDict(extra="forbid", frozen=True, json_schema_extra=untitled)

    question: Prose = Field(description="The question, as asked again.")
    answer: Prose = Field(description="The owner's answer.")
    matches: bool = Field(description="Whether the answer matches the plan; a wrong one reopens that part.")


class UnderstandingRecord(BaseModel):
    """`changes/understanding/v1`: how the owner showed he understands a change's data or UX.

    The owner's sign-off decision references this record in its `refs`.
    """

    model_config = ConfigDict(extra="forbid", frozen=True, json_schema_extra=untitled)

    tag: Literal["changes/understanding/v1"] = Field(alias="$type", description="The record's type.")
    change: Annotated[str, Field(json_schema_extra={"format": "board-ref", "x-board-type": CHANGE_TYPE})] = Field(
        description="The change whose plan was checked."
    )
    subject: Literal["data", "ux"] = Field(description="What was checked: the plan's Data or its UX.")
    playback: Prose = Field(
        description="What the agent played back: what is stored, where, who shares it, what users see."
    )
    corrections: list[Prose] = Field(description="Each correction the owner made to the play-back.")
    questions: list[Asked] = Field(min_length=1, description="Each question asked, in order.")
    mismatches: list[Mismatch] = Field(description="Each answer that differed from the plan, and how it was settled.")
    parked: list[Prose] = Field(description="What was named and left out of scope.")
    rechecks: list[Recheck] = Field(description="The questions asked again before review.")


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


class BriefUse(Strict):
    """One record the plan uses, and one line saying what it holds."""

    ref: str
    line: str


class BriefOutput(Strict):
    """What an agent needs to start a change, in at most 600 words.

    `plan` is the newest plan's text, or `null` when the change has no plan
    record. `uses` gives one line per record it uses, in order, and
    `left_out` counts those the word limit left out. `controls` is the
    command that lists the controls on the change's repository.
    `brief_line`, `board-brief: <path>`, comes first in the output; a
    builder's first message starts with it, and the write guard reads it.
    """

    brief_line: str
    path: str
    plan: str | None
    uses: list[BriefUse]
    left_out: int
    phase: Phase
    instruction: str
    controls: str


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
    task: LogicalPath | SkipJsonSchema[None] = Field(
        default=None,
        description="The task the change does, such as /repos/notes-personal/tasks/<slug>.md; when left out, "
        "tasks.add adds one titled as the change.",
    )
    task_repo: LogicalPath | SkipJsonSchema[None] = Field(
        default=None,
        description="The notes repo tasks.add writes the new task to, such as /repos/notes-personal; the caller's "
        "git remote chooses it when left out.",
    )


class PrototypeInput(Strict):
    """A prototype that answers one question: its task under `parent`, its change in build and its brief."""

    model_config = ConfigDict(json_schema_extra=shipped())

    mount: LogicalPath = Field(description="The repository's mount, such as /repos/board.")
    question: str = Field(pattern=r"\S", description="The question the prototype answers, which is its whole plan.")
    parent: LogicalPath = Field(
        description="The task or project the prototype's task goes under, such as "
        "/repos/notes-personal/tasks/<slug>.md."
    )
    title: str | SkipJsonSchema[None] = Field(
        default=None,
        pattern="[A-Za-z0-9]",
        description="What the prototype is, in a few words; the question when left out.",
    )
    name: str | SkipJsonSchema[None] = Field(
        default=None,
        pattern="^[a-z0-9][a-z0-9-]{0,59}$",
        description="The change's name, so it lives at /changes/<name>; made from the title when left out.",
    )


class VerdictInput(Strict):
    """What a prototype found, which ends it."""

    model_config = ConfigDict(json_schema_extra=shipped())

    path: LogicalPath = Field(description="The prototype's logical path, such as /changes/step-list.")
    state: Ended = Field(description="settled when it answered its question, abandoned when the owner says so.")
    verdict: str = Field(pattern=r"\S", description="What it found, in one sentence.")
    artifact: SharedUrl | SkipJsonSchema[None] = Field(
        default=None, description="The URL of the artifact version the verdict means."
    )


class PathInput(Strict):
    """One change, by path."""

    path: LogicalPath = Field(description="The change's logical path, such as /changes/suite-timeout.")


class AttachInput(Strict):
    """Add a record, ticket or pull reference, or set the task, route, head commit, issue or pull request."""

    model_config = ConfigDict(json_schema_extra=shipped(ATTACH_SOMETHING))

    path: LogicalPath = Field(description="The change's logical path, such as /changes/suite-timeout.")
    ref: LogicalPath | SkipJsonSchema[None] = Field(
        default=None, description="A record the change produced, such as /suites/board-tests/runs/<id>."
    )
    ticket: LogicalPath | SkipJsonSchema[None] = Field(
        default=None, description=f"A ticket the change works on, a {' or '.join(TICKET_TYPES)} record."
    )
    pull: LogicalPath | SkipJsonSchema[None] = Field(
        default=None, description=f"A pull request of the change, a {' or '.join(PULL_TYPES)} record."
    )
    task: LogicalPath | SkipJsonSchema[None] = Field(
        default=None, description=f"The task the change does, a {TASK_TYPE} record."
    )
    route: Route | SkipJsonSchema[None] = Field(default=None, description="The route: owner, ask-first or unowned.")
    head: Commit | SkipJsonSchema[None] = Field(default=None, description="The commit under test and review.")
    issue: Url | SkipJsonSchema[None] = Field(default=None, description="The URL of the issue the change answers.")
    pull_request: Url | SkipJsonSchema[None] = Field(default=None, description="The pull request's URL.")


class OpenInput(Strict):
    """The changes that are not done or dropped, by path."""

    model_config = ConfigDict(json_schema_extra=shipped())

    kind: Kind | SkipJsonSchema[None] = Field(
        default=None,
        description="Only changes of this kind; prototype also lists the settled and abandoned ones.",
    )
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
    task: str
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


class Prototype(Strict):
    """What a prototype asks and found, and the branch its code is on."""

    question: str | None
    state: State | None
    verdict: str | None
    verdict_artifact: str | None
    branch: str


class OpenChange(Strict):
    """A change that is not done or dropped, its task and kind, and, for a prototype, what it asks and found."""

    path: str
    title: str
    mount: str
    phase: Phase
    route: Route | None
    task: str | None
    kind: Kind
    prototype: Prototype | None


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


class PrototypeOutput(Strict):
    """The prototype started: its change, task and branch, and the brief its builder starts from."""

    path: str
    task: str
    branch: str
    brief: BriefOutput


class VerdictOutput(Strict):
    """The prototype ended, and the phase that ended it: done when settled, dropped when abandoned."""

    path: str
    state: Ended
    phase: Phase


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
UNDERSTANDING_SCHEMA_PATH = f"{UNDERSTANDING_TYPE}.schema.json"
PLAN_SCHEMA_PATH = f"{PLAN_TYPE}.schema.json"
