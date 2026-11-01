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
        None,
        min_length=1,
        json_schema_extra={"x-board-prose": True},
        description="Its title when the record was written; later edits on GitHub leave it as is.",
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
