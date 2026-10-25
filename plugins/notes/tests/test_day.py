"""The person's day starts at `BOARD_DAY_START`, or at midnight."""

import datetime as dt

import pytest

from board_notes.day import day_start, today

SEVEN = dt.timedelta(hours=7)
LONDON = dt.timezone(dt.timedelta(hours=1))


def test_a_moment_before_the_day_starts_belongs_to_yesterday_and_one_at_the_start_to_today() -> None:
    assert today(dt.datetime(2026, 10, 25, 1, 0, tzinfo=LONDON), SEVEN) == dt.date(2026, 10, 24)
    assert today(dt.datetime(2026, 10, 25, 6, 59, tzinfo=LONDON), SEVEN) == dt.date(2026, 10, 24)
    assert today(dt.datetime(2026, 10, 25, 7, 0, tzinfo=LONDON), SEVEN) == dt.date(2026, 10, 25)
    assert today(dt.datetime(2026, 10, 25, 0, 0, tzinfo=LONDON), dt.timedelta()) == dt.date(2026, 10, 25)


def test_the_day_starts_at_board_day_start_and_at_midnight_without_it(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv("BOARD_DAY_START", raising=False)
    assert day_start() == dt.timedelta()
    monkeypatch.setenv("BOARD_DAY_START", "07:30")
    assert day_start() == dt.timedelta(hours=7, minutes=30)
