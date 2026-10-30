"""The sweep's sections at a fixed moment, from tasks as `available` reads them, with no `board` run."""

import datetime as dt
import json
from typing import Any

from board_tasks.available import Task, read, sweep

ZONE = dt.timezone(dt.timedelta(hours=3))
SEVEN = dt.timedelta(hours=7)
# 25 October 2026, 10:00 in a zone three hours ahead of UTC.
NOW = dt.datetime(2026, 10, 25, 10, 0, tzinfo=ZONE)


def task(title: str, **fields: Any) -> Task:  # noqa: ANN401 - the task's other fields
    """An open task in the board's own tree, reviewed on the moment's day unless `fields` says otherwise."""
    values: dict[str, Any] = {
        "due": None,
        "defer": None,
        "from_hour": None,
        "reviewed": dt.date(2026, 10, 25),
        "contexts": list[str](),
        "waits": False,
        "owner": None,
    } | fields
    return Task(repo="/", path=f"/tasks/{title}.md", title=title, **values)


def sections(
    tasks: list[Task],
    now: dt.datetime = NOW,
    start: dt.timedelta = SEVEN,
    blocked: dict[str, list[str]] | None = None,
) -> dict[str, list[str]]:
    """The titles in each section."""
    swept = sweep(tasks, blocked or {}, now, start)
    return {name: [item["title"] for item in section["tasks"]] for name, section in swept.items()}


def test_a_task_deferred_to_a_later_date_or_moment_waits_and_one_deferred_to_now_or_earlier_is_available() -> None:
    tasks = [
        task("tomorrow", defer="2026-10-26"),
        task("today", defer="2026-10-25"),
        task("in-an-hour", defer="2026-10-25T11:00:00+03:00"),
        task("an-hour-ago", defer="2026-10-25T09:00:00+03:00"),
    ]
    assert sections(tasks)["available"] == ["an-hour-ago", "today"]


def test_a_horizon_counts_from_reviewed_and_without_reviewed_defers_nothing() -> None:
    tasks = [
        task("soon-from-today", defer="soon"),
        task("soon-from-long-ago", defer="soon", reviewed=dt.date(2026, 10, 1)),
        task("not-today-from-yesterday", defer="not-today", reviewed=dt.date(2026, 10, 24)),
        task("someday-unreviewed", defer="someday", reviewed=None),
    ]
    assert sections(tasks) == {
        "available": ["not-today-from-yesterday", "someday-unreviewed", "soon-from-long-ago"],
        "due_soon": [],
        "stale": [],
        "held": [],
    }


def test_an_hour_before_the_day_starts_belongs_to_the_day_before() -> None:
    evening = [task("evening", from_hour=17), task("dawn", from_hour=3)]
    one_am = dt.datetime(2026, 10, 26, 1, 0, tzinfo=ZONE)
    assert sections(evening, one_am)["available"] == ["evening"], "17:00 on the 25th has passed; 03:00 on the 26th not"
    assert sections(evening, NOW)["available"] == [], "neither 17:00 nor the next 03:00 has come"
    assert sections(evening, one_am, dt.timedelta())["available"] == [], "with a midnight start, 01:00 is a new day"
    four_am = dt.datetime(2026, 10, 26, 4, 0, tzinfo=ZONE)
    assert sections(evening, four_am)["available"] == ["dawn", "evening"]


def test_a_blocked_task_waits_and_its_blocker_counts_it() -> None:
    tasks = [task("blocker"), task("waiter", waits=True)]
    swept = sweep(tasks, {"/tasks/waiter.md": ["/tasks/blocker.md"]}, NOW, SEVEN)
    shown = {item["title"]: (item["waiting"], item["blocked_by"]) for item in swept["available"]["tasks"]}
    assert shown == {"blocker": (1, [])}


def test_due_soon_holds_every_open_task_due_within_seven_days_or_overdue_whether_available_or_not() -> None:
    tasks = [
        task("overdue", due=dt.date(2026, 10, 1)),
        task("in-seven-days", due=dt.date(2026, 11, 1), defer="someday"),
        task("in-eight-days", due=dt.date(2026, 11, 2)),
    ]
    assert sections(tasks)["due_soon"] == ["in-seven-days", "overdue"]


def test_a_task_not_available_is_stale_without_reviewed_or_reviewed_over_two_weeks_ago_unless_deferred() -> None:
    old = dt.date(2026, 10, 10)
    tasks = [
        task("blocked-old", reviewed=old, waits=True),
        task("blocked-recent", reviewed=dt.date(2026, 10, 11), waits=True),
        task("blocked-unreviewed", reviewed=None, waits=True),
        task("deferred-old", reviewed=old, defer="2026-12-01"),
        task("deferred-unreviewed", reviewed=None, defer="2026-12-01"),
        task("someday-old", reviewed=old, defer="someday"),
        task("available-old", reviewed=old),
    ]
    blocked = {item.path: ["/tasks/x.md"] for item in tasks if item.waits}
    assert sections(tasks, blocked=blocked)["stale"] == ["blocked-old", "blocked-unreviewed"]


def test_a_section_lists_at_most_a_hundred_tasks_by_repo_then_title_and_counts_them_all() -> None:
    tasks = [task(f"t{number:03d}") for number in range(101, -1, -1)]
    tasks.append(
        Task(
            "/repos/a", "/repos/a/tasks/a.md", "a", None, None, None, dt.date(2026, 10, 25), [], waits=False, owner=None
        )
    )
    swept = sweep(tasks, {}, NOW, SEVEN)["available"]
    assert (len(swept["tasks"]), swept["total"]) == (100, 103)
    assert [item["title"] for item in swept["tasks"][:3]] == ["t000", "t001", "t002"]
    assert swept["tasks"][-1]["title"] == "t099"
    assert "a" not in [item["title"] for item in swept["tasks"]], "/ sorts before /repos/a"


def test_a_date_time_with_a_lowercase_t_or_without_an_offset_is_a_moment_too() -> None:
    tasks = [
        task("lower-later", defer="2026-10-25t11:00:00+03:00"),
        task("lower-earlier", defer="2026-10-25t09:00:00+03:00"),
        task("local-later", defer=(NOW + dt.timedelta(hours=1)).astimezone().replace(tzinfo=None).isoformat()),
    ]
    assert sections(tasks)["available"] == ["lower-earlier"]


def test_a_date_defer_ends_when_the_persons_day_starts_and_not_at_midnight() -> None:
    tasks = [task("monday", defer="2026-10-26")]
    assert sections(tasks, now=dt.datetime(2026, 10, 26, 5, 0, tzinfo=ZONE))["available"] == []
    assert sections(tasks, now=dt.datetime(2026, 10, 26, 8, 0, tzinfo=ZONE))["available"] == ["monday"]


def test_a_task_whose_dates_cannot_be_read_is_degraded_and_one_whose_dates_can_is_read() -> None:
    def row(defer: str, due: str | None = None) -> list[Any]:
        body = {"$type": "tasks/task/v2", "title": defer, "status": "open", "defer": defer, "reviewed": "2026-10-25"}
        return ["", f"/tasks/{defer}.md", None, json.dumps(body | ({} if due is None else {"due": due}))]

    rows = [row("2026-10-25T25:00:00Z"), row("tomorrow"), row("soon", due="2026-13-01"), row("2026-10-25t10:00:00Z")]
    tasks, degraded = read(rows, ["/"])
    assert ([item.title for item in tasks], degraded) == (["2026-10-25t10:00:00Z"], 3)


def test_an_owner_keeps_the_tasks_it_or_nobody_holds_and_held_lists_its_tasks_that_are_not_available() -> None:
    tasks = [
        task("mine", owner="lead", due=dt.date(2026, 10, 26)),
        task("nobodys", due=dt.date(2026, 10, 26)),
        task("theirs", owner="other", due=dt.date(2026, 10, 26)),
        task("mine-deferred", owner="lead", defer="2026-10-30"),
        task("mine-blocked", owner="lead", waits=True),
        task("mine-later-today", owner="lead", from_hour=17),
        task("theirs-deferred", owner="other", defer="2026-10-30"),
    ]
    blocked = {"/tasks/mine-blocked.md": ["/tasks/theirs.md"]}
    swept = sweep(tasks, blocked, NOW, SEVEN, "lead")
    assert {name: [item["title"] for item in section["tasks"]] for name, section in swept.items()} == {
        "available": ["mine", "nobodys"],
        "due_soon": ["mine", "nobodys"],
        "stale": [],
        "held": ["mine-blocked", "mine-deferred", "mine-later-today"],
    }
    assert [swept[name]["total"] for name in ("available", "held")] == [2, 3]
    unowned = sections(tasks, blocked=blocked)
    assert (unowned["available"], unowned["held"]) == (["mine", "nobodys", "theirs"], [])
