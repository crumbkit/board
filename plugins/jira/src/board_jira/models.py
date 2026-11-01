"""The records and the capability's input and output, from which the plugin's schemas are generated.

A call reads them as plain JSON, which Board has validated against these
schemas, so a call loads no pydantic (STANDARDS.md P3).
"""

from typing import Literal, cast

from board_plugin.model import Strict, unset
from pydantic import ConfigDict, Field
from pydantic.config import JsonDict
from pydantic.json_schema import SkipJsonSchema

from board_jira.ref import KEY, SITE


def untitled(schema: JsonDict) -> None:
    """Drop the titles pydantic gives the record and each property (docs/architecture/records/shapes.md, rule 11)."""
    schema.pop("title", None)
    for field in cast("dict[str, JsonDict]", schema.get("properties", {})).values():
        field.pop("title", None)


class IssueRecord(Strict):
    """`jira/issue/v1`: one Jira issue, at `/jira/<site>/<key>`."""

    model_config = ConfigDict(json_schema_extra=untitled)

    tag: Literal["jira/issue/v1"] = Field(alias="$type", description="The record's type.")
    site: str = Field(pattern=f"^{SITE}$", description="The site name before .atlassian.net, such as acme.")
    key: str = Field(pattern=f"^{KEY}$", description="The issue key, such as ACME-123.")
    url: str = Field(
        pattern=rf"^https://{SITE}\.atlassian\.net/browse/{KEY}$",
        description="The address of the issue's page, such as https://acme.atlassian.net/browse/ACME-123.",
    )
    title: str | SkipJsonSchema[None] = Field(
        default=None,
        json_schema_extra=unset,
        min_length=1,
        description="The issue's summary when the record was written; absent when it could not be read.",
    )


class ConfigRecord(Strict):
    """`jira/config/v1`: the default site, at `/jira/config`."""

    model_config = ConfigDict(json_schema_extra=untitled)

    tag: Literal["jira/config/v1"] = Field(alias="$type", description="The record's type.")
    site: str = Field(
        pattern=f"^{SITE}$", description="The site `jira.ref` uses when no --site is given, such as acme."
    )


class RefInput(Strict):
    """The issue to record."""

    key: str = Field(description="The issue key, such as ACME-123.")
    site: str | SkipJsonSchema[None] = Field(
        default=None,
        json_schema_extra=unset,
        description="The site name before .atlassian.net, such as acme; the config record's site when left out.",
    )


class RefOutput(Strict):
    """The issue's record, whether this call wrote it, and why it has no title when it has none.

    `title_note` is null when the record has a title or was already there.
    """

    path: str
    site: str
    key: str
    url: str
    title: str | None
    written: bool
    title_note: str | None
