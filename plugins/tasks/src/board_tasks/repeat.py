"""When a repeating task comes back, and what completing a task writes (README.md, Writing tasks)."""

import calendar
import contextlib
import datetime as dt
from typing import Any

from board_tasks.available import HORIZONS

WEEK = ("mon", "tue", "wed", "thu", "fri", "sat", "sun")
FRIDAY, SATURDAY = 4, 5


def following(rule: str, after: dt.date) -> dt.date:
    """The first date after `after` that `rule` names. A month day past the month's last is its last day."""
    kind, _, value = rule.partition("-")
    if kind == "daily":
        return after + dt.timedelta(days=1)
    if kind == "weekdays":
        return after + dt.timedelta(days={FRIDAY: 3, SATURDAY: 2}.get(after.weekday(), 1))
    if kind == "weekly":
        return after + dt.timedelta(days=(WEEK.index(value) - after.weekday() - 1) % 7 + 1)
    if kind == "monthly":
        months = [(after.year, after.month), (after.year + after.month // 12, after.month % 12 + 1)]
        return next(date for date in (clamped(*month, int(value)) for month in months) if date > after)
    month, day = (int(part) for part in value.split("-"))
    return next(date for date in (clamped(year, month, day) for year in (after.year, after.year + 1)) if date > after)


def clamped(year: int, month: int, day: int) -> dt.date:
    """The `day` of the month, or the month's last day when it has fewer."""
    return dt.date(year, month, min(day, calendar.monthrange(year, month)[1]))


def completed(record: dict[str, Any], today: dt.date) -> dict[str, str]:
    """The fields completing the task sets on `today`.

    A task without `repeat` is done. A repeating one stays open, deferred to its next occurrence, and never
    to an earlier date than its `defer`. The occurrence counts from the date `defer` ends, so a missed one
    comes back: a date as it is, or a horizon counted from `reviewed`, as `tasks.available` counts it. Without
    either, it counts from today. An `ephemeral` task counts from the later of that date and today, so a missed
    occurrence is not kept.
    """
    rule = record.get("repeat")
    if rule is None:
        return {"status": "done"}
    defer, start = str(record.get("defer") or ""), today
    with contextlib.suppress(ValueError):
        if defer in HORIZONS:
            start = dt.date.fromisoformat(str(record.get("reviewed"))) + dt.timedelta(days=HORIZONS[defer])
        else:
            start = dt.date.fromisoformat(defer[:10])
    if record.get("ephemeral") is True:
        start = max(start, today)
    return {"defer": following(str(rule), start).isoformat()}
