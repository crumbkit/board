"""The capabilities' inputs and outputs, and the record type the plugin ships, from which its schemas are generated.

A call reads them as plain JSON, which Board has validated against these
schemas, so a call loads no pydantic (STANDARDS.md P3).
"""

import datetime as dt
from typing import Annotated, Literal

from board_plugin.model import Strict, unset
from pydantic import Field
from pydantic.json_schema import SkipJsonSchema

REPO = Field(
    default=None,
    json_schema_extra=unset,
    pattern="^/",
    description=(
        "The notes repo: / for the board's own tree, or a mount's path such as /repos/notes-personal; "
        "chosen by the caller's git remote when left out."
    ),
)
REMOTE = Field(
    default=None,
    json_schema_extra=unset,
    min_length=1,
    description=(
        "A git remote, or owner/name, to route by in place of the origin of the directory board was started in."
    ),
)


class AddInput(Strict):
    """Append text under a `## HH:MM` heading to today's daily note, or to a topic's note."""

    text: str = Field(pattern=r"\S", description="The Markdown to append; not only whitespace.")
    repo: str | SkipJsonSchema[None] = REPO
    remote: str | SkipJsonSchema[None] = REMOTE
    topic: str | SkipJsonSchema[None] = Field(
        default=None,
        json_schema_extra=unset,
        min_length=1,
        description="The title of the note to append to, at notes/<slug>.md; today's daily note when left out.",
    )


class AddOutput(Strict):
    """Where the text went.

    `path` is the logical path written and `commit` the repo's commit that
    holds it: the mount's pin, or the board's commit for the board's own tree,
    null when the note was left uncommitted.
    `candidates` is null, or the notes repos covering the remote when the note
    went to the personal repo's `inbox/` because not exactly one covers it.
    `parked` is null, or the Board code that kept the text out of the note: it
    went to the sibling `<name>.agent.md` instead. `unpushed` is null when the
    commit reached the repo's remote, or the code of the sync that did not
    push it: `notes::repo::not_synced` for the board's own tree.
    """

    repo: str
    path: str
    commit: str | None
    candidates: list[str] | None
    parked: str | None
    unpushed: str | None


class DailyInput(Strict):
    """The daily note of one day, as the repo's remote holds it."""

    repo: str | SkipJsonSchema[None] = REPO
    remote: str | SkipJsonSchema[None] = REMOTE
    date: dt.date | SkipJsonSchema[None] = Field(
        default=None,
        json_schema_extra=unset,
        description="The day, as YYYY-MM-DD; today when left out, by the caller's time zone and day_start.",
    )


class DailyOutput(Strict):
    """The daily note.

    `text` is null when the note does not exist, or when it is private:
    its frontmatter has a `private` key whose value is not plainly false
    (`false`, `no`, `off` or `0`), as Reflect keeps private notes from AI. A read
    creates no note. `commit` is the repo's commit read, null for the board's
    own tree, which is read from its working tree. `unsynced` is null when the
    repo synced first, or the code of the sync that failed, and the note is
    then as this clone holds it: `notes::repo::not_synced` for the board's own
    tree.
    """

    repo: str
    path: str
    exists: bool
    private: bool
    text: str | None
    commit: str | None
    unsynced: str | None


class RepoRecord(Strict):
    """`notes/repo/v1`, at `notes-repo.json` in a notes repo's root: which work the repo's notes are about.

    `covers` names GitHub repositories as `owner/name`, or all of an owner's
    as `owner/*`. A note from a checkout whose remote one notes repo covers
    goes to that repo. `personal` marks the one notes repo that takes, in its
    `inbox/`, the notes no single repo covers.
    """

    type_: Literal["notes/repo/v1"] = Field(alias="$type")
    covers: list[Annotated[str, Field(pattern=r"^[A-Za-z0-9-]+/(\*|[A-Za-z0-9._-]+)$")]] = Field(
        description="GitHub repositories as owner/name, or owner/* for all of an owner's; an empty list covers none."
    )
    personal: bool | SkipJsonSchema[None] = Field(
        default=None,
        json_schema_extra=unset,
        description="Whether this repo takes, in inbox/, the notes no single repo covers.",
    )
