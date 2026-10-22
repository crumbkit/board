"""The `to` capability's input and output, from which the manifest's schemas are generated.

A call reads them as plain JSON, which Board has validated against these
schemas, so a call loads no pydantic (STANDARDS.md P3).
"""

from board_plugin.model import Strict
from pydantic import Field


class ToInput(Strict):
    """Copy the entry at `path`, its children and every entry its records reference into the board mounted at `to`."""

    path: str = Field(
        min_length=1,
        json_schema_extra={"format": "board-path"},
        description="The record or file to share, such as /evals/x; its children under /evals/x/ go with it.",
    )
    to: str = Field(
        min_length=1,
        json_schema_extra={"format": "board-path"},
        description="The mount record of the board to copy into, such as /team; the share writes through it.",
    )
    dry_run: bool = Field(
        default=False,
        description=(
            "List what would be copied and the plugin's own refusals, and write nothing, when true. Board's checks"
            " for a write through a mount run only on the real call."
        ),
    )
    local_attachments: bool = Field(
        default=False,
        description="Copy an attachment whose bytes are on this machine, which other readers cannot fetch, when true.",
    )


class ToOutput(Strict):
    """What the share copied, or would copy on a dry run.

    `repository` is the repository the mount reaches. `commit` is the commit
    in it that holds the copies, which the mount's pin now names, null when
    nothing was written. `types` are the types whose schemas were added in
    that commit. `copied` lists at most 100
    paths; `total` counts them all, and `truncated` says whether some were left
    out. `unchanged` counts the paths the destination already held equally.
    """

    repository: str
    commit: str | None
    types: list[str]
    copied: list[str]
    total: int
    truncated: bool
    unchanged: int
