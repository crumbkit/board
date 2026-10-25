"""When a repeating task comes back, and what completing a task writes."""

import datetime as dt

import pytest

from board_tasks.repeat import completed, following

D = dt.date


@pytest.mark.parametrize(
    ("rule", "after", "then"),
    [
        ("daily", D(2026, 12, 31), D(2027, 1, 1)),
        ("weekdays", D(2026, 10, 23), D(2026, 10, 26)),
        ("weekdays", D(2026, 10, 24), D(2026, 10, 26)),
        ("weekdays", D(2026, 10, 25), D(2026, 10, 26)),
        ("weekdays", D(2026, 10, 26), D(2026, 10, 27)),
        ("weekly-mon", D(2026, 10, 26), D(2026, 11, 2)),
        ("weekly-sun", D(2026, 10, 26), D(2026, 11, 1)),
        ("monthly-15", D(2026, 10, 14), D(2026, 10, 15)),
        ("monthly-15", D(2026, 10, 15), D(2026, 11, 15)),
        ("monthly-31", D(2027, 1, 31), D(2027, 2, 28)),
        ("monthly-31", D(2027, 2, 28), D(2027, 3, 31)),
        ("monthly-31", D(2026, 12, 31), D(2027, 1, 31)),
        ("annually-02-29", D(2027, 1, 1), D(2027, 2, 28)),
        ("annually-02-29", D(2027, 2, 28), D(2028, 2, 29)),
        ("annually-10-25", D(2026, 10, 25), D(2027, 10, 25)),
    ],
)
def test_the_next_occurrence_is_the_first_after_the_date_with_a_month_day_clamped_to_its_last(
    rule: str, after: dt.date, then: dt.date
) -> None:
    assert following(rule, after) == then


TODAY = D(2026, 10, 28)


def test_a_task_without_repeat_is_done() -> None:
    assert completed({"status": "open"}, TODAY) == {"status": "done"}


def test_a_repeating_task_is_deferred_to_its_next_occurrence_never_earlier_than_its_defer_and_stays_open() -> None:
    assert completed({"repeat": "daily"}, TODAY) == {"defer": "2026-10-29"}
    assert completed({"repeat": "daily", "defer": "soon"}, TODAY) == {"defer": "2026-10-29"}, "no reviewed"
    soon = {"repeat": "daily", "defer": "soon", "reviewed": "2026-10-27"}
    assert completed(soon, TODAY) == {"defer": "2026-11-17"}, "soon ends 20 days after reviewed, on 16 November"
    assert completed(soon | {"reviewed": "2026-09-01"}, TODAY) == {"defer": "2026-09-22"}, "a missed horizon"
    later = {"repeat": "daily", "defer": "2026-11-05"}
    assert completed(later, TODAY) == {"defer": "2026-11-06"}
    assert completed(later | {"ephemeral": True}, TODAY) == {"defer": "2026-11-06"}


def test_a_missed_occurrence_comes_back_one_by_one_unless_the_task_is_ephemeral() -> None:
    missed = {"repeat": "daily", "defer": "2026-10-25"}
    assert completed(missed, TODAY) == {"defer": "2026-10-26"}, "the next missed day"
    assert completed(missed | {"defer": "2026-10-25T09:00:00+02:00"}, TODAY) == {"defer": "2026-10-26"}
    assert completed(missed | {"ephemeral": True}, TODAY) == {"defer": "2026-10-29"}, "no backlog"
