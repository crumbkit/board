"""Extraction: one row per task record, keyed by its blob (STANDARDS.md P5)."""

import sqlite3
from contextlib import closing

from board_tasks.models import TaskRecord
from board_tasks.protocol import ExtractRequest

SCHEMA = """
CREATE TABLE IF NOT EXISTS tasks (
    blob TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    status TEXT NOT NULL,
    due TEXT
) WITHOUT ROWID
"""


def extract(request: ExtractRequest) -> list[str]:
    """Replace the row of each record's blob, and name every blob handled."""
    with closing(sqlite3.connect(request.board.database)) as database, database:
        database.execute(SCHEMA)
        for record in request.records:
            task = TaskRecord.model_validate(record.record)
            database.execute("DELETE FROM tasks WHERE blob = ?", (record.blob,))
            database.execute(
                "INSERT INTO tasks (blob, title, status, due) VALUES (?, ?, ?, ?)",
                (record.blob, task.title, task.status, task.due.isoformat() if task.due else None),
            )
    return [record.blob for record in request.records]
