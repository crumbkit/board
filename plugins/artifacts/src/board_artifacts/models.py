"""The artifact and version records, and the plugin's inputs and outputs.

The records' schemas, which the plugin ships, and the manifest's schemas are
generated from these models.
"""

import datetime
from collections.abc import Callable
from typing import Annotated, Any, Literal, cast, override

from board_plugin.model import Strict
from pydantic import AwareDatetime, BaseModel, ConfigDict, Field
from pydantic.config import JsonDict
from pydantic.json_schema import GenerateJsonSchema, SkipJsonSchema

ARTIFACT_TYPE = "artifacts/artifact/v1"
VERSION_TYPE = "artifacts/version/v1"

# The most artifacts the Artifact tool's `list` action returns: the most one import reads and `list` returns.
LIST_LIMIT = 200
LIST_DEFAULT = 50

# A claude.ai artifact's link. Its last segment names the artifact's records, so it holds no `/` or `.`.
ARTIFACT_URL = r"^https://claude\.ai/(code/)?artifact/[A-Za-z0-9_-]+$"

ArtifactUrl = Annotated[
    str,
    Field(
        pattern=ARTIFACT_URL,
        description="The artifact's claude.ai link, such as https://claude.ai/artifact/AbCdEf0123456789xyzQ.",
    ),
]
Account = Annotated[
    str,
    Field(min_length=1, description="The email address of the claude.ai account that published the artifact."),
]


def artifact_id(url: str) -> str:
    """The last segment of an artifact's link, which names its records."""
    return url.rsplit("/", 1)[-1]


class _Untitled(GenerateJsonSchema):
    """Schemas without property titles, as Board's shape rules want (docs/architecture/records/shapes.md 11)."""

    @override
    def field_title_should_be_set(self, schema: Any) -> bool:
        return False


def schema_of(model: type[BaseModel]) -> dict[str, Any]:
    """`model`'s JSON schema, by alias and without property titles."""
    return model.model_json_schema(by_alias=True, schema_generator=_Untitled)


def shipped(extra: JsonDict | None = None) -> Callable[[JsonDict], None]:
    """A schema hook: no other field, no `null` default for a field typed otherwise, and `extra` keywords."""

    def hook(schema: JsonDict) -> None:
        schema["additionalProperties"] = False
        for field in cast("dict[str, JsonDict]", schema.get("properties", {})).values():
            field.pop("default", None)
        schema.update(extra or {})

    return hook


def unset_defaults(schema: JsonDict) -> None:
    """A schema hook: no `null` default for a field the caller may leave out."""
    for field in cast("dict[str, JsonDict]", schema.get("properties", {})).values():
        if field.get("default") is None:
            field.pop("default", None)


class Record(BaseModel):
    """A record this plugin reads and ships a schema for.

    The schema refuses any other field, but a board may keep a looser schema
    of its own, so the plugin reads a record ignoring the rest.
    """

    model_config = ConfigDict(extra="ignore", frozen=True)


class ArtifactRecord(Record):
    """`artifacts/artifact/v1` at `/artifacts/<id>`: one claude.ai artifact, written once.

    `<id>` is the last segment of `url`. `first_seen` is the first publish the
    mod saw, or for an import the listing row's `updatedAt`, or the import time
    when the row has none. The newest version record under `<id>/versions/` is
    the artifact's current version.
    """

    model_config = ConfigDict(json_schema_extra=shipped())

    tag: Literal["artifacts/artifact/v1"] = Field(alias="$type", description="The record's type.")
    url: ArtifactUrl
    account: Account
    first_seen: AwareDatetime = Field(description="When the board first saw the artifact, with a time zone.")


# A version without a copy has no hash, and a version with one has its hash.
COPY_RULES: JsonDict = {
    "if": {"required": ["copy"]},
    "then": {"not": {"required": ["sha256"]}},
    "else": {"required": ["sha256"]},
}


class VersionRecord(Record):
    """`artifacts/version/v1` at `/artifacts/<id>/versions/<key>`: one version, written once.

    The page is beside it as `<key>.html` or `<key>.md`. `<key>` is `version`,
    the version id the Artifact tool returned, or, when it returned none, the
    first 16 hex digits of `sha256`, and the record leaves `version` out.
    `title` and `description` are the artifact's at this version. A version
    with no local page, such as one an Artifact type wrote, has no copy and
    says `copy: "none"`, and then has no `sha256`.
    """

    model_config = ConfigDict(json_schema_extra=shipped(COPY_RULES))

    tag: Literal["artifacts/version/v1"] = Field(alias="$type", description="The record's type.")
    version: SkipJsonSchema[None] | Annotated[str, Field(pattern="^[A-Za-z0-9_-]+$")] = Field(
        default=None, description="The version id the Artifact tool returned, such as 1793473476-1c42."
    )
    title: SkipJsonSchema[None] | str = Field(default=None, description="The artifact's title at this version.")
    description: SkipJsonSchema[None] | str = Field(
        default=None, description="The description the publish passed, when it passed one."
    )
    sha256: SkipJsonSchema[None] | Annotated[str, Field(pattern="^[0-9a-f]{64}$")] = Field(
        default=None, description="The SHA-256 of the page copied beside this record, in hex."
    )
    source: SkipJsonSchema[None] | Annotated[str, Field(min_length=1)] = Field(
        default=None, description="The local file the publish sent, when a publish wrote this version."
    )
    session: str = Field(min_length=1, description="The Claude Code session that published or read this version.")
    at: AwareDatetime = Field(description="When the session published or read this version, with a time zone.")
    copy_: SkipJsonSchema[None] | Literal["none"] = Field(
        default=None, alias="copy", description="none when the version has no local page, so nothing was copied."
    )


class ListInput(Strict):
    """The artifacts in the board, newest first."""

    model_config = ConfigDict(json_schema_extra=unset_defaults)

    account: SkipJsonSchema[None] | Annotated[str, Field(min_length=1)] = Field(
        default=None, description="Only the artifacts of this account, by its email address."
    )
    limit: int = Field(
        default=LIST_DEFAULT,
        ge=1,
        le=LIST_LIMIT,
        description=f"The most artifacts to return, 1 to {LIST_LIMIT}; {LIST_DEFAULT} when left out.",
    )


class Current(Strict):
    """An artifact's newest version: its record's path, version id, title and when it was recorded.

    `version` is null for a version keyed by its page's hash.
    """

    path: str
    version: str | None
    title: str | None
    at: datetime.datetime


class Listed(Strict):
    """One artifact, by its record's path, with its newest version, or null when it has none yet."""

    path: str
    url: str
    account: str
    first_seen: datetime.datetime
    current: Current | None


class ListOutput(Strict):
    """Artifacts, newest first by their newest version, or by `first_seen` for one with no version.

    `total` counts every artifact that matched and `truncated` says whether the
    limit left some out. `unreadable` counts the artifact and version records
    Board marks degraded or this plugin cannot read, which were left out.
    """

    artifacts: list[Listed]
    total: int
    truncated: bool
    unreadable: int


class Row(BaseModel):
    """One artifact of the Artifact tool's list result. The tool's other fields are kept out of the record."""

    model_config = ConfigDict(extra="allow", frozen=True, json_schema_extra=unset_defaults)

    url: ArtifactUrl
    title: str = Field(description="The artifact's title in the listing.")
    updated_at: SkipJsonSchema[None] | AwareDatetime = Field(
        default=None, alias="updatedAt", description="When the artifact was last updated, with a time zone."
    )


class Listing(BaseModel):
    """The Artifact tool's `list` result, unchanged: `{artifacts, truncated?, total?}` and the tool's other fields.

    Only a listing of the account's own artifacts is admitted, so a `scope` of
    `shared` or `all` is refused.
    """

    model_config = ConfigDict(extra="allow", frozen=True, json_schema_extra=unset_defaults)

    artifacts: list[Row] = Field(
        max_length=LIST_LIMIT,
        description=f"The listed artifacts, at most {LIST_LIMIT}, most recently updated first.",
    )
    truncated: SkipJsonSchema[None] | bool = Field(
        default=None, description="Whether the account has more artifacts than the listing holds."
    )
    total: SkipJsonSchema[None] | Annotated[int, Field(ge=0)] = Field(
        default=None, description="How many artifacts the account has."
    )
    scope: SkipJsonSchema[None] | Literal["mine"] = Field(
        default=None, description="Absent in a listing of the account's own artifacts; mine is the same."
    )


class ImportInput(Strict):
    """Record each artifact of an Artifact tool listing that the board does not have."""

    account: Account
    listing: Listing = Field(
        description="The Artifact tool's list result, unchanged. Board reads stdin only when no flag is given, so "
        'give the whole input as JSON on stdin: {"account": "owner@example.com", "listing": {...}}.'
    )


class Truncated(Strict):
    """A listing that left artifacts out: how many it listed, how many the account has, and what that means."""

    listed: int
    total: int | None
    message: str


class ImportOutput(Strict):
    """The artifact records written, by path, how many the board already had, and whether the listing was cut short."""

    written: list[str]
    known: int
    truncated: Truncated | None
