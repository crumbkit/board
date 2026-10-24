"""The capabilities' inputs and outputs, from which the manifest's schemas are generated.

A call reads them as plain JSON, which Board has validated against these
schemas, so a call loads no pydantic (STANDARDS.md P3).
"""

import datetime as dt

from board_plugin.model import Strict
from pydantic import Field
from pydantic.config import JsonDict
from pydantic.json_schema import SkipJsonSchema


def unset(schema: JsonDict) -> None:
    """Leave out the `null` default of a string the caller may leave out."""
    schema.pop("default", None)


VAULT = Field(
    default=None,
    json_schema_extra=unset,
    pattern="^[a-z0-9-]+$",
    description="The vault mounted at /vaults/<vault>; the only one mounted there when left out.",
)


class AddInput(Strict):
    """Append text under a `## HH:MM` heading to today's daily note, or to a topic's note."""

    text: str = Field(pattern=r"\S", description="The Markdown to append; not only whitespace.")
    vault: str | SkipJsonSchema[None] = VAULT
    topic: str | SkipJsonSchema[None] = Field(
        default=None,
        json_schema_extra=unset,
        min_length=1,
        description="The title of the note to append to, at notes/<slug>.md; today's daily note when left out.",
    )


class AddOutput(Strict):
    """Where the text went.

    `path` is the logical path written and `commit` the vault's commit that
    holds it, which the mount's pin names. `parked` is null, or the Board code
    that kept the text out of the note: it went to the sibling `<name>.agent.md`
    instead. `unpushed` is null when the commit reached the vault's remote, or
    the Board code of the sync that did not push it.
    """

    vault: str
    path: str
    commit: str
    parked: str | None
    unpushed: str | None


class DailyInput(Strict):
    """The daily note of one day, as the vault's remote holds it."""

    vault: str | SkipJsonSchema[None] = VAULT
    date: dt.date | SkipJsonSchema[None] = Field(
        default=None,
        json_schema_extra=unset,
        description="The day, as YYYY-MM-DD; today, in this machine's time zone, when left out.",
    )


class DailyOutput(Strict):
    """The daily note.

    `text` is null when the note does not exist, or when it is private:
    its frontmatter has a `private` key whose value is not plainly false
    (`false`, `no`, `off` or `0`), as Reflect keeps private notes from AI. A read
    creates no note. `commit` is the vault's commit read. `unsynced` is null
    when the vault synced first, or the Board code of the sync that failed,
    and the note is then as this clone holds it.
    """

    vault: str
    path: str
    exists: bool
    private: bool
    text: str | None
    commit: str
    unsynced: str | None
