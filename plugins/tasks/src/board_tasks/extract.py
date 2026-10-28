"""Extraction: one row per task, keyed by its blob (STANDARDS.md P5)."""

import json
import sqlite3
import sys
from contextlib import closing

from board_plugin.protocol import ExtractRecord, ExtractRequest
from pydantic import ValidationError

from board_tasks.models import Task
from board_tasks.tables import SCHEMA


def extract(request: ExtractRequest) -> list[str]:
    """Replace the rows of each record's blob, and name every blob handled.

    A board's own schema for a task type may admit a record this plugin
    cannot read. That record gets no row and is still handled, so it is not
    sent again; one line on stderr says why.
    """
    with closing(sqlite3.connect(request.board.database)) as database, database:
        database.executescript(SCHEMA)
        for record in request.records:
            database.execute("DELETE FROM tasks_v2 WHERE blob = ?", (record.blob,))
            try:
                _v2(database, record)
            except ValidationError as error:
                sys.stderr.write(f"tasks: {record.path} is not a task this plugin reads: {error.errors()}\n")
    return [record.blob for record in request.records]


def _v2(database: sqlite3.Connection, record: ExtractRecord) -> None:
    """The rows of a `tasks/task/v2` task, its values as the frontmatter wrote them."""
    task = Task.model_validate(record.record)
    defer = record.record.get("defer")
    database.execute(
        "INSERT INTO tasks_v2 (blob, title, status, due, defer, from_hour, reviewed, contexts, waits, owner)"
        " VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        (
            record.blob,
            task.title,
            task.status,
            task.due.isoformat() if task.due else None,
            None if defer is None else str(defer),
            task.from_hour,
            task.reviewed.isoformat() if task.reviewed else None,
            json.dumps(task.contexts),
            len(task.after),
            task.owner,
        ),
    )
