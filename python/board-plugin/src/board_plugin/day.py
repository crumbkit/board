"""The person's day, which starts at `BOARD_DAY_START`, such as `07:00`, or at midnight.

Board reads `day_start` from its known boards file, refuses a value that is
not `HH:MM`, and passes it to each plugin it calls. A note at 01:00 with a
day that starts at 07:00 belongs to yesterday.
"""

import datetime as dt
import os


def day_start() -> dt.timedelta:
    """How long after midnight the person's day starts, as Board passed it; midnight when it passed none."""
    hours, _, minutes = os.environ.get("BOARD_DAY_START", "00:00").partition(":")
    return dt.timedelta(hours=int(hours), minutes=int(minutes))


def today(now: dt.datetime, start: dt.timedelta) -> dt.date:
    """The date of the day `now` falls in, for a day that starts `start` after midnight."""
    return (now - start).date()
