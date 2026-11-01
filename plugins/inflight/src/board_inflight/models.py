"""The capability's input and output, from which the manifest's schemas are generated.

A call reads them as plain JSON, which Board has validated against these
schemas, so a call loads no pydantic (STANDARDS.md P3).
"""

import datetime as dt
from typing import Annotated, Any, Literal, cast

from board_plugin.model import Strict
from pydantic import ConfigDict, Field
from pydantic.config import JsonDict
from pydantic.json_schema import SkipJsonSchema

# The most entries each part returns by default, and at most.
LIMIT = 20
MAX = 100


def shipped(schema: JsonDict) -> None:
    """A schema hook: no `null` default for an input field typed otherwise."""
    for field in cast("dict[str, JsonDict]", schema.get("properties", {})).values():
        field.pop("default", None)


class NowInput(Strict):
    """What is open or running right now."""

    model_config = ConfigDict(json_schema_extra=shipped)

    holder: Annotated[str, Field(pattern=r"\S")] | SkipJsonSchema[None] = Field(
        default=None, description="Only the changes this session holds, their agents, and the agents it started."
    )
    stopped_at: dt.datetime | SkipJsonSchema[None] = Field(
        default=None,
        alias="stopped-at",
        description="When the sessions stopped, with a time zone; an agent with no run that started before it is"
        " stalled.",
    )
    limit: int = Field(
        default=LIMIT,
        ge=1,
        le=MAX,
        description=f"The most entries each part returns, 1 to {MAX}; {LIMIT} when left out.",
    )


class Refusal(Strict):
    """Why a part or a step derivation is missing: the code and message of the call that refused."""

    code: str
    message: str


class Agent(Strict):
    """A subagent with no run yet: its record, its prompt beside it, and whether it is running or stalled."""

    path: str
    prompt: str
    session: str
    cwd: str
    started: dt.datetime
    works_on: str | None
    state: Literal["running", "stalled"]


class Artifact(Strict):
    """An artifact one of whose versions names the change in `for`, with its newest version's title and time."""

    path: str
    url: str
    title: str | None
    at: dt.datetime


class Change(Strict):
    """An open change, its next step and who takes it, its pull request, its agents and its artifacts.

    `next` and `actor` are null when `changes.next` refused, and `refused`
    says why. `pr_state` is the command that says whether the pull request
    merged, since this query makes no network call. `agents_left_out` and
    `artifacts_left_out` count what the limit left out.
    """

    path: str
    title: str
    repo: str
    branch: str
    holder: str | None
    worktree: str | None
    next: str | None
    actor: Literal["agent", "reviewer", "owner"] | None
    refused: Refusal | None
    pull_request: str | None
    pr_state: str | None
    agents: list[Agent]
    agents_left_out: int
    artifacts: list[Artifact]
    artifacts_left_out: int


class Changes(Strict):
    """Open changes, by path. `unreadable` counts the ones Board marks degraded."""

    changes: list[Change]
    total: int
    left_out: int
    unreadable: int


class Unattached(Strict):
    """Agents whose `works_on` names no open change, or who have none, oldest first.

    `unreadable` counts the agent records, here or under a change, that Board
    marks degraded or that this plugin cannot read.
    """

    agents: list[Agent]
    total: int
    left_out: int
    unreadable: int


class Waiting(Strict):
    """Entries from another plugin's capability, as it gives them, or why it refused."""

    entries: list[dict[str, Any]]
    total: int
    left_out: int
    refused: Refusal | None


class NowOutput(Strict):
    """What is open or running: changes with their agents, unattached agents, questions and evals.

    Each part returns at most `limit` entries and counts in `left_out` those
    it left out. `questions` are the entries of `plans.open`, and `evals` those
    of `evals.open`.
    """

    changes: Changes
    unattached: Unattached
    questions: Waiting
    evals: Waiting
