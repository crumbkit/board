"""Extraction: one row per task record, keyed by its blob, and one per `after` entry of a v2 task (STANDARDS.md P5)."""

import json
import sqlite3
import sys
from contextlib import closing

from board_plugin.protocol import ExtractRecord, ExtractRequest
from pydantic import ValidationError

from board_tasks.models import TASK_V2_TYPE, TaskRecord, TaskV2
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
            for table in ("tasks", "tasks_v2"):
                database.execute(f"DELETE FROM {table} WHERE blob = ?", (record.blob,))  # noqa: S608 - fixed names
            try:
                if record.record_type == TASK_V2_TYPE:
                    _v2(database, record)
                else:
                    task = TaskRecord.model_validate(record.record)
                    database.execute(
                        "INSERT INTO tasks (blob, title, status, due) VALUES (?, ?, ?, ?)",
                        (record.blob, task.title, task.status, task.due.isoformat() if task.due else None),
                    )
            except ValidationError as error:
                sys.stderr.write(f"tasks: {record.path} is not a task this plugin reads: {error.errors()}\n")
    return [record.blob for record in request.records]


def _v2(database: sqlite3.Connection, record: ExtractRecord) -> None:
    """The rows of a `tasks/task/v2` task, its values as the frontmatter wrote them."""
    task = TaskV2.model_validate(record.record)
    defer = record.record.get("defer")
    database.execute(
        "INSERT INTO tasks_v2 (blob, title, status, due, defer, from_hour, reviewed, contexts, waits)"
        " VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
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
        ),
    )
