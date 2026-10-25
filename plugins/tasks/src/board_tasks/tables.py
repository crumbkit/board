"""The plugin's tables in its own database, which extraction fills and `available` reads through `board query`."""

import sqlite3
from contextlib import closing

SCHEMA = """
CREATE TABLE IF NOT EXISTS tasks_v2 (
    blob TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    status TEXT NOT NULL,
    due TEXT,
    defer TEXT,
    from_hour INTEGER,
    reviewed TEXT,
    contexts TEXT NOT NULL,
    waits INTEGER NOT NULL
) WITHOUT ROWID;
"""


def make(database: str) -> None:
    """Make the tables in `database` if they are missing.

    Board makes a plugin's database empty and runs extraction only for a board that holds a task, so a call
    on a board with none makes the tables itself, or its query would name a table that does not exist.
    """
    with closing(sqlite3.connect(database)) as opened, opened:
        opened.executescript(SCHEMA)
