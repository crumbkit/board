"""The plugin's inputs and outputs, from which its manifest's schemas are generated."""

import datetime
from typing import Literal

from board_plugin.protocol import Strict
from pydantic import BaseModel, ConfigDict


class TaskRecord(BaseModel):
    """The fields of an `lr/task/v1` record that the plugin reads.

    A user record's own schema decides its fields (STANDARDS.md 3), so the
    rest are ignored rather than refused.
    """

    model_config = ConfigDict(extra="ignore", frozen=True)

    title: str
    status: Literal["open", "done"]
    due: datetime.date | None = None


class DueInput(Strict):
    """Open tasks due before this date."""

    before: datetime.date


class DueTask(Strict):
    """An open task and when it is due."""

    path: str
    title: str
    due: datetime.date


class DueOutput(Strict):
    """Open tasks due before the date asked for, by due date then path."""

    tasks: list[DueTask]
    truncated: bool
