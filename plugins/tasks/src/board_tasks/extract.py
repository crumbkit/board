"""Extraction: one row per task record, keyed by its blob (STANDARDS.md P5)."""

import sqlite3
import sys
from contextlib import closing

from board_plugin.protocol import ExtractRequest
from pydantic import ValidationError

from board_tasks.models import TaskRecord

SCHEMA = """
CREATE TABLE IF NOT EXISTS tasks (
    blob TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    status TEXT NOT NULL,
    due TEXT
) WITHOUT ROWID
"""


def extract(request: ExtractRequest) -> list[str]:
    """Replace the row of each record's blob, and name every blob handled.

    A board's own schema for `lr/task/v1` may admit a record this plugin
    cannot read. That record gets no row and is still handled, so it is not
    sent again; one line on stderr says why.
    """
    with closing(sqlite3.connect(request.board.database)) as database, database:
        database.execute(SCHEMA)
        for record in request.records:
            database.execute("DELETE FROM tasks WHERE blob = ?", (record.blob,))
            try:
                task = TaskRecord.model_validate(record.record)
            except ValidationError as error:
                sys.stderr.write(f"tasks: {record.path} is not a task this plugin reads: {error.errors()}\n")
                continue
            database.execute(
                "INSERT INTO tasks (blob, title, status, due) VALUES (?, ?, ?, ?)",
                (record.blob, task.title, task.status, task.due.isoformat() if task.due else None),
            )
    return [record.blob for record in request.records]
