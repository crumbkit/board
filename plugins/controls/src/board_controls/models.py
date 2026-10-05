"""The control record, and the plugin's inputs and outputs.

The record's schema, which the plugin ships, and the manifest's schemas are
generated from these models.
"""

from typing import Annotated, Literal

from pydantic import Field

from board_controls.protocol import Strict

CONTROL_TYPE = "controls/control/v1"

# A reference Board resolves and checks: an absolute logical path to any entry.
BoardRef = Annotated[str, Field(json_schema_extra={"format": "board-ref"})]


class Budget(Strict):
    """A limit on the words or bytes of every file the control applies to, summed.

    Words are separated by any whitespace.
    """

    budget: Literal["words", "bytes"]
    max: int = Field(ge=0)


class ControlRecord(Strict):
    """`controls/control/v1`: a requirement on files, why it exists, and optionally a budget a program checks.

    It applies to the entries `applies_to` references and to every entry of
    the board whose path matches one of `paths`, SQLite GLOB patterns such as
    `docs/*.md`.
    """

    tag: Literal["controls/control/v1"] = Field(alias="$type")
    requirement: str
    purpose: str
    applies_to: list[BoardRef] = Field(default_factory=list[str])
    paths: list[str] = Field(default_factory=list[str])
    rule: Budget | None = None


class ForPathInput(Strict):
    """The controls that apply to this logical path."""

    path: str = Field(pattern="^/")


class Applying(Strict):
    """A control that applies to the path asked about."""

    control: str
    requirement: str
    purpose: str
    rule: Budget | None


class ForPathOutput(Strict):
    """The controls that apply to the path, by the control's path."""

    controls: list[Applying]


class CheckInput(Strict):
    """Check every budget control in the board."""


class Checked(Strict):
    """A budget control and what its files use."""

    control: str
    requirement: str
    budget: Literal["words", "bytes"]
    max: int
    used: int
    files: list[str]


class CheckOutput(Strict):
    """Every budget control, by the control's path; each is within its budget."""

    controls: list[Checked]
