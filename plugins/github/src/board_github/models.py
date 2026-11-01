"""The records and the capability's input and output, from which the shipped schemas are generated.

A call reads them as plain JSON, which Board has validated against these
schemas, so a call loads no pydantic (STANDARDS.md P3).
"""

from typing import Literal, cast

from board_plugin.model import Strict
from pydantic import BaseModel, ConfigDict, Field
from pydantic.config import JsonDict
from pydantic.json_schema import SkipJsonSchema

from board_github.objects import ISSUE_TYPE, PULL_TYPE, REF_PATTERN

PULL_SCHEMA_PATH = f"{PULL_TYPE}.schema.json"
ISSUE_SCHEMA_PATH = f"{ISSUE_TYPE}.schema.json"


def shaped(schema: JsonDict) -> None:
    """A schema hook: no `title` and no `null` default on a property (docs/architecture/records/shapes.md)."""
    for field in cast("dict[str, JsonDict]", schema.get("properties", {})).values():
        field.pop("title", None)
        field.pop("default", None)


class _Object(BaseModel):
    """The fields a pull request and an issue record share."""

    model_config = ConfigDict(extra="forbid", frozen=True, json_schema_extra=shaped)

    owner: str = Field(
        pattern="^[^A-Z]+$", description="The repository's owner, a user or organisation, in lower case, such as octo."
    )
    repo: str = Field(pattern="^[^A-Z]+$", description="The repository's name, in lower case, such as widgets.")
    number: int = Field(ge=1, description="The number GitHub gave it in the repository.")
    url: str = Field(pattern="^https://", description="Its page on GitHub.")
    title: str | SkipJsonSchema[None] = Field(
        None, min_length=1, description="Its title when the record was written; later edits on GitHub leave it as is."
    )


class PullRecord(_Object):
    """`github/pull/v1`: one pull request, at `/gh/<owner>/<repo>/pulls/<number>`."""

    tag: Literal["github/pull/v1"] = Field(alias="$type", description="The record's type.")


class IssueRecord(_Object):
    """`github/issue/v1`: one issue, at `/gh/<owner>/<repo>/issues/<number>`."""

    tag: Literal["github/issue/v1"] = Field(alias="$type", description="The record's type.")


class RefInput(Strict):
    """Record one pull request or issue."""

    ref: str = Field(
        pattern=REF_PATTERN, description="The pull request or issue as owner/repo#number, such as octo/widgets#12."
    )


class RefOutput(Strict):
    """The record for the reference, and whether this call wrote it.

    `written` is false when the record was already there; nothing was asked of
    GitHub and nothing was written. `title` is null when the record holds none.
    """

    path: str
    type: Literal["github/pull/v1", "github/issue/v1"]
    written: bool
    url: str
    title: str | None


def optional(schema: JsonDict) -> None:
    """A schema hook: an input the caller may leave out has no `null` default."""
    for field in cast("dict[str, JsonDict]", schema.get("properties", {})).values():
        field.pop("default", None)


def title_or_body(schema: JsonDict) -> None:
    """A schema hook: as `optional`, and the input must give a title, a body or both."""
    optional(schema)
    schema["anyOf"] = [{"required": ["title"]}, {"required": ["body"]}]


class PushInput(Strict):
    """Push the branch checked out to origin, once the boundary passes the commits it would send."""


class PrCreateInput(Strict):
    """Push the branch checked out and open a pull request from it, once the boundary passes both."""

    model_config = ConfigDict(json_schema_extra=optional)

    title: str = Field(min_length=1, description="The pull request's title.")
    body: str = Field(description="The pull request's body, in Markdown.")
    base: str | SkipJsonSchema[None] = Field(
        None, min_length=1, description="The branch to merge into; GitHub's default branch when left out."
    )
    draft: bool | SkipJsonSchema[None] = Field(None, description="Whether to open it as a draft.")


class PrEditInput(Strict):
    """Change a pull request's title or body, once the boundary passes the new text."""

    model_config = ConfigDict(json_schema_extra=title_or_body)

    number: int = Field(ge=1, description="The pull request's number in the checkout's repository.")
    title: str | SkipJsonSchema[None] = Field(None, min_length=1, description="The new title.")
    body: str | SkipJsonSchema[None] = Field(None, description="The new body, in Markdown.")


class CommentInput(Strict):
    """Comment on a pull request or issue, once the boundary passes the comment."""

    number: int = Field(ge=1, description="The pull request's or issue's number in the checkout's repository.")
    body: str = Field(min_length=1, description="The comment, in Markdown.")


class IssueCreateInput(Strict):
    """Open an issue in the checkout's repository, once the boundary passes it."""

    title: str = Field(min_length=1, description="The issue's title.")
    body: str = Field(description="The issue's body, in Markdown.")


class SentOutput(Strict):
    """The crossing the boundary passed, and what GitHub returned for what was sent.

    `note` says why nothing was checked, when no boundary covered the content.
    `sent` is what GitHub returned, also recorded on the crossing.
    """

    digest: str
    crossing: str
    note: str | None
    sent: str


class PushOutput(SentOutput):
    """The branch pushed and the commits sent, oldest first; `sent` is git's line for the branch."""

    branch: str
    commits: list[str]


class PrCreateOutput(PushOutput):
    """The pushed branch and the pull request opened from it; `sent` is its URL and `path` its record."""

    number: int
    path: str


class IssueCreateOutput(SentOutput):
    """The issue opened; `sent` is its URL and `path` its record."""

    number: int
    path: str


class InstallHookInput(Strict):
    """Install the git pre-push hook in the checkout Board was started in."""


class InstallHookOutput(Strict):
    """Where the hook is, and whether this call wrote it; false when the same hook was already there."""

    path: str
    installed: bool


class PrePushInput(Strict):
    """What git gives its pre-push hook: the remote and the refs it pushes. The URL is in BOARD_GITHUB_PUSH_URL."""

    remote: str = Field(description="The remote's name, or the URL git was given in its place.")
    refs: str = Field(
        description=(
            "What git writes on the hook's stdin: one `<local ref> <local sha> <remote ref> <remote sha>` per line."
        )
    )


class Checked(Strict):
    """One ref's check against one destination."""

    ref: str
    to: str
    digest: str
    note: str | None


class PrePushOutput(Strict):
    """Each ref checked against each GitHub destination; empty when the push goes nowhere on GitHub or sends nothing."""

    checked: list[Checked]


class CoversInput(Strict):
    """Name the boundaries that cover a write from the checkout, checking nothing."""

    model_config = ConfigDict(json_schema_extra=optional)

    action: str = Field(
        pattern="^[a-z-]+$", description="The github action that sends the write, such as push or pr-create."
    )
    remote: str | SkipJsonSchema[None] = Field(
        None, min_length=1, description="The remote a push names; the branch's push remote, else origin, when left out."
    )
    repo: str | SkipJsonSchema[None] = Field(
        None, pattern="^[^/]+/[^/]+$", description="The owner/repo the write names, in place of the remote."
    )


class Covered(Strict):
    """One destination a boundary covers, and the boundaries."""

    to: str
    boundaries: list[str]


class CoversOutput(Strict):
    """Each destination of the write that a boundary covers; empty when none is."""

    covered: list[Covered]
