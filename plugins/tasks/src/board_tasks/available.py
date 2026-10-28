"""The `available` capability: the open tasks to sweep now, those due soon, and the stale ones (README.md)."""

import datetime as dt
import json
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass
from typing import Any, cast

from board_plugin import repos
from board_plugin.board import Board
from board_plugin.day import day_start, today
from board_plugin.protocol import CallRequest, PluginError

from board_tasks import tables
from board_tasks.models import FIELDS, TASK_TYPE, TREE_CODES

HORIZONS = {"not-today": 1, "not-this-week": 6, "soon": 20, "someday": 90}
DUE_SOON = dt.timedelta(days=7)
STALE = dt.timedelta(days=14)
# The most tasks a section lists; `total` counts the rest (STANDARDS 23).
SHOWN = 100
# The seconds one repo's sync may take. A sync still running then is left to finish on its own, and the repo
# is read at its pin.
SYNC_DEADLINE = 5.0
SYNC_TIMEOUT = "tasks::repo::sync_timeout"
# Each Markdown file under a `tasks/` directory of its repo that may be an open task: its repo's root, its
# path, its Board code, and the open task extracted from it, or NULLs when none was.
TASK_COLUMNS = (
    "substr(e.logical, 1, length(e.logical) - length(e.path))",
    "e.logical",
    "e.code",
    "t.title",
    "t.due",
    "t.defer",
    "t.from_hour",
    "t.reviewed",
    "t.contexts",
    "t.waits",
    "t.owner",
)
TASKS = f"""
SELECT {", ".join(TASK_COLUMNS)}
FROM entries AS e LEFT JOIN p_tasks.tasks_v2 AS t ON t.blob = e.blob AND e.type = 'tasks/task/v2'
WHERE e.path GLOB '*.md' AND ('/' || e.path) GLOB '*/tasks/*' AND (t.blob IS NULL OR t.status = 'open')
ORDER BY e.logical
"""  # noqa: S608 - the columns interpolated are constants
# The notes repos and the task files in one query, since each `board query` costs a `board` run. The repos'
# rows are padded with NULLs to the tasks' width.
LISTING = f"""
SELECT 'repo', *{", NULL" * (len(TASK_COLUMNS) - len(repos.COLUMNS))} FROM ({repos.REPOS})
UNION ALL
SELECT 'task', * FROM ({TASKS})
"""  # noqa: S608 - the queries interpolated are constants


@dataclass(frozen=True)
class Task:
    """An open task, as extracted, with the repo that holds it."""

    repo: str
    path: str
    title: str
    due: dt.date | None
    defer: str | None
    from_hour: int | None
    reviewed: dt.date | None
    contexts: list[str]
    waits: bool
    owner: str | None


def available(request: CallRequest) -> dict[str, object]:
    """Sync the repos in scope, then sort their open tasks into the sweep's sections.

    The caller's origin is read while the first query runs. The tasks and what they wait on are read while
    the repos sync, at the pins the call started with. Only when a sync moves a clone past its pin does the
    call move that pin and read them again, so a call where nothing changed starts two `board` runs in turn:
    one query, then the syncs and the refs together.
    """
    board = Board(request.board.root, "tasks")
    tables.make(request.board.database)
    name, given = request.input.get("repo"), request.input.get("remote")
    with ThreadPoolExecutor() as pool:
        origin = pool.submit(repos.caller_origin) if name is None and given is None else None
        found, rows = listing(board)
        remote, unread = (None if given is None else str(given), None) if origin is None else origin.result()
        roots = repos.scope(board.plugin, found, None if name is None else str(name), remote, unread)
        mounted = [(at, commit, clone) for at, commit, clone, _ in found if at in roots and clone is not None]
        tasks, degraded = read(rows, roots)
        reports = pool.submit(resolve, board, tasks)
        synced = list(pool.map(sync, [board] * len(mounted), [clone for _, _, clone in mounted]))
        blocked, crossing = reports.result()
    unsynced: list[dict[str, str]] = []
    moved = False
    # Each pin writes the board, so the pins move in turn, once the reads that ran beside the syncs are done.
    for (at, commit, clone), (code, head) in zip(mounted, synced, strict=True):
        failed = code
        if code is None and head is not None and head != commit:
            failed = pin(repos.Repo(board, at, clone))
            moved = moved or failed is None
        if failed is not None:
            unsynced.append({"repo": at, "code": failed})
    if moved:
        tasks, degraded = read(listing(board)[1], roots)
        blocked, crossing = resolve(board, tasks)
    context, owner = request.input.get("context"), request.input.get("owner")
    if context is not None:
        tasks = [task for task in tasks if context in task.contexts]
    sections = sweep(tasks, blocked, dt.datetime.now().astimezone(), day_start(), cast("str | None", owner))
    return {
        **sections,
        "repos": roots,
        "degraded": degraded,
        "unsynced": unsynced,
        "cross_repo": crossing,
        "origin_unread": unread,
    }


def listing(board: Board) -> tuple[list[repos.Listed], list[list[Any]]]:
    """The notes repos and the task files, from one `board query`."""
    rows = board.rows(LISTING)
    width = 1 + len(repos.COLUMNS)
    found = repos.parsed(board.plugin, [row[1:width] for row in rows if row[0] == "repo"])
    return found, [row[1:] for row in rows if row[0] == "task"]


def resolve(board: Board, tasks: list[Task]) -> tuple[dict[str, list[str]], list[dict[str, str]]]:
    """What each task with an `after` waits on, by the task's path, and the `after` entries not followed.

    One `board refs` answers every task. An `after` that reaches a task in another notes repo than its own,
    such as a task in the board's own tree naming one in a mount, is listed and never blocks.
    """
    waits = [task for task in tasks if task.waits]
    blocked: dict[str, list[str]] = {}
    crossing: list[dict[str, str]] = []
    for task, report in zip(waits, board.refs([task.path for task in waits], fields=FIELDS), strict=True):
        blocked[task.path], across = blockers(report or {}, task.repo)
        crossing += [{"path": task.path, "after": target} for target in across]
    return blocked, crossing


def blockers(report: dict[str, Any], repo: str) -> tuple[list[str], list[str]]:
    """The tasks a task in `repo` reaches through `after` that are not done, and the targets in another repo.

    Each blocker is listed once, by its path in the board, and an unresolved one as written. A target is done
    when it is a `tasks/task/v2` record whose status is `done` and which check degrades for nothing but the
    tree. A target held by another commit than the task, or the board's own tree for a mount, is in another
    repo. Board displays a target in a mount by its path in that mount, so the repo's root is put in front.
    """
    own: dict[str, Any] = report.get("location") or {}
    source = own.get("source")
    found: list[str] = []
    across: list[str] = []
    for outgoing in report.get("from", []):
        if outgoing.get("link") or not str(outgoing.get("pointer", "")).startswith("/after/"):
            continue
        location = outgoing["location"]
        if location is not None and location.get("source") != source:
            across.append(str(outgoing["target"]))
            continue
        display = str(outgoing["target"] if location is None else location["display"])
        display = display if repo == "/" else repo + display
        values = {value["pointer"]: value.get("value") for value in outgoing.get("values", [])}
        code = outgoing.get("target_code")
        done = values.get("/$type") == TASK_TYPE and values.get("/status") == "done" and code in (None, *TREE_CODES)
        if not done and display not in found:
            found.append(display)
    return found, across


def sync(board: Board, clone: str) -> tuple[str | None, str | None]:
    """Sync the clone without pushing: the code of a sync that failed or ran past its deadline, and HEAD after it.

    HEAD is None unless the sync succeeded. It is compared with the mount's pin, which `board mount update`
    moves to the ref the mount follows. A clone that has another branch checked out than that ref differs
    every time, so its pin moves on every call, which is correct but slower.
    """
    try:
        report = board.document("-C", clone, "sync", "--no-push", timeout=SYNC_DEADLINE)
    except PluginError as error:
        if error.code == f"{board.plugin}::board::timed_out":
            return SYNC_TIMEOUT, None
        return str(error.fields.get("code", error.code)), None
    data: dict[str, Any] = report.get("data") or {}
    head = data.get("new")
    return None, None if head is None else str(head)


def pin(repo: repos.Repo) -> str | None:
    """Move the mount's pin to its clone's HEAD: None, or the code of the failure that left the pin where it was."""
    try:
        repo.pin()
    except PluginError as error:
        return str(error.fields.get("code", error.code))
    return None


def read(rows: list[list[Any]], roots: list[str]) -> tuple[list[Task], int]:
    """The open tasks in `roots`, and how many of their task files are degraded or hold a date that cannot be read."""
    tasks: list[Task] = []
    degraded = 0
    for root, path, code, title, due, defer, hour, reviewed, contexts, waits, owner in rows:
        at = repos.root(str(root))
        if at not in roots:
            continue
        if title is None or (code is not None and code not in TREE_CODES):
            degraded += 1
            continue
        try:
            if defer is not None and defer not in HORIZONS:
                until(defer)
            task = Task(
                at,
                str(path),
                str(title),
                _date(due),
                defer,
                hour,
                _date(reviewed),
                json.loads(contexts),
                bool(waits),
                owner,
            )
        except ValueError:
            degraded += 1
            continue
        tasks.append(task)
    return tasks, degraded


def sweep(
    tasks: list[Task], blocked: dict[str, list[str]], now: dt.datetime, start: dt.timedelta, owner: str | None = None
) -> dict[str, dict[str, Any]]:
    """The sections of the sweep at `now`, for a day that starts `start` after midnight. It does no I/O.

    With an `owner`, every section keeps the tasks that owner or nobody holds, and `held` lists the ones that
    owner holds that are not available. `waiting` still counts every task.
    """
    day = today(now, start)
    waiting: dict[str, int] = {}
    for targets in blocked.values():
        for target in targets:
            waiting[target] = waiting.get(target, 0) + 1
    shown: dict[str, list[dict[str, Any]]] = {"available": [], "due_soon": [], "stale": [], "held": []}
    for task in sorted(tasks, key=lambda task: (task.repo, task.title, task.path)):
        if owner is not None and task.owner not in (None, owner):
            continue
        parked = deferred(task, day, now)
        free = not parked and not blocked.get(task.path) and opened(task.from_hour, day, now, start)
        old = not parked and (task.reviewed is None or task.reviewed < day - STALE)
        item = {
            "repo": task.repo,
            "path": task.path,
            "title": task.title,
            "due": None if task.due is None else task.due.isoformat(),
            "defer": task.defer,
            "reviewed": None if task.reviewed is None else task.reviewed.isoformat(),
            "contexts": task.contexts,
            "owner": task.owner,
            "blocked_by": blocked.get(task.path, []),
            "waiting": waiting.get(task.path, 0),
        }
        if free:
            shown["available"].append(item)
        if task.due is not None and task.due <= day + DUE_SOON:
            shown["due_soon"].append(item)
        if not free and old:
            shown["stale"].append(item)
        if not free and owner is not None and task.owner == owner:
            shown["held"].append(item)
    return {name: {"tasks": items[:SHOWN], "total": len(items)} for name, items in shown.items()}


def deferred(task: Task, day: dt.date, now: dt.datetime) -> bool:
    """Whether the task is deferred past `now`. A horizon counts from `reviewed`; without one, it defers nothing."""
    if task.defer is None:
        return False
    if task.defer in HORIZONS:
        return task.reviewed is not None and day < task.reviewed + dt.timedelta(days=HORIZONS[task.defer])
    moment = until(task.defer)
    return now < moment if isinstance(moment, dt.datetime) else day < moment


def until(defer: str) -> dt.date | dt.datetime:
    """The date or moment a dated `defer` names. A value longer than a date is a date-time, with `T` or `t`.

    A date-time without an offset is taken in local time. A value that is neither raises ValueError.
    """
    if len(defer) <= len("2026-10-25"):
        return dt.date.fromisoformat(defer)
    moment = dt.datetime.fromisoformat(defer)
    return moment if moment.tzinfo is not None else moment.astimezone()


def opened(hour: int | None, day: dt.date, now: dt.datetime, start: dt.timedelta) -> bool:
    """Whether the person's `day` has reached `hour`: an hour before the day starts falls on the next date."""
    if hour is None:
        return True
    midnight = dt.datetime.combine(day, dt.time(), now.tzinfo)
    at = midnight + dt.timedelta(hours=hour)
    return now >= (at + dt.timedelta(days=1) if at < midnight + start else at)


def _date(value: object) -> dt.date | None:
    return None if value is None else dt.date.fromisoformat(str(value))
