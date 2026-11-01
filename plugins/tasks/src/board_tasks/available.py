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

from board_tasks import models
from board_tasks.models import FIELDS, TASK_TYPE, TREE_CODES
from board_tasks.tree import Forest, Node, forest, place, waits_on

HORIZONS = {"not-today": 1, "not-this-week": 6, "soon": 20, "someday": 90}
DUE_SOON = dt.timedelta(days=7)
STALE = dt.timedelta(days=14)
# The most tasks a section lists; `total` counts the rest (STANDARDS 23).
SHOWN = 100
# The seconds one repo's sync may take. A sync still running then is left to finish on its own, and the repo
# is read at its pin.
SYNC_DEADLINE = 5.0
SYNC_TIMEOUT = "tasks::repo::sync_timeout"
# Each Markdown file under a `tasks/` directory of its repo that may be a task: its repo's root, its path, its
# Board code, and its frontmatter as JSON when it is a `tasks/task/v2` record that parses, or NULL.
TASK_COLUMNS = ("substr(e.logical, 1, length(e.logical) - length(e.path))", "e.logical", "e.code", "b.body")
TASKS = f"""
SELECT {", ".join(TASK_COLUMNS)}
FROM entries AS e LEFT JOIN bodies AS b ON b.blob = e.blob AND e.type = 'tasks/task/v2'
WHERE e.path GLOB '*.md' AND ('/' || e.path) GLOB '*/tasks/*'
ORDER BY e.logical
"""  # noqa: S608 - the columns interpolated are constants
# The notes repos and the task files in one query, since each `board query` costs a `board` run. The narrower
# of the two is padded with NULLs to the other's width.
WIDTH = max(len(repos.COLUMNS), len(TASK_COLUMNS))
LISTING = f"""
SELECT 'repo', *{", NULL" * (WIDTH - len(repos.COLUMNS))} FROM ({repos.REPOS})
UNION ALL
SELECT 'task', *{", NULL" * (WIDTH - len(TASK_COLUMNS))} FROM ({TASKS})
"""  # noqa: S608 - the queries interpolated are constants


@dataclass(frozen=True)
class Task:
    """A task, with the repo that holds it. `defer` is as the frontmatter wrote it.

    `waits` and `linked` say whether it has an `after` and a `parent`, which `board refs` resolves.
    """

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
    status: str = "open"
    linked: bool = False
    kind: str = "task"
    brief: str | None = None


@dataclass(frozen=True)
class Links:
    """What `board refs` resolved for tasks: each one's open `after` targets and parent, and the crossings."""

    blocked: dict[str, list[str]]
    parents: dict[str, str]
    crossing: list[dict[str, str]]


def available(request: CallRequest) -> dict[str, object]:
    """Sync the repos in scope, then sort their open tasks into the sweep's sections.

    The caller's origin is read while the first query runs. The tasks and what they wait on are read while
    the repos sync, at the pins the call started with. Only when a sync moves a clone past its pin does the
    call move that pin and read them again, so a call where nothing changed starts two `board` runs in turn:
    one query, then the syncs and the refs together.
    """
    board = Board(request.board.root, "tasks")
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
        links = reports.result()
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
        links = resolve(board, tasks)
    nodes, tree = grown(tasks, links)
    tasks = [task for task in tasks if task.status == "open"]
    blocked = {task.path: waited(links, tree, nodes, task.path) for task in tasks}
    places = {task.path: place(tree, nodes, task.path) for task in tasks}
    context, owner = request.input.get("context"), request.input.get("owner")
    if context is not None:
        tasks = [task for task in tasks if context in task.contexts]
    now = dt.datetime.now().astimezone()
    sections = sweep(tasks, blocked, now, day_start(), cast("str | None", owner), places=places)
    return {
        **sections,
        "repos": roots,
        "degraded": degraded,
        "unsynced": unsynced,
        "cross_repo": links.crossing,
        "origin_unread": unread,
    }


def listing(board: Board) -> tuple[list[repos.Listed], list[list[Any]]]:
    """The notes repos and the task files, from one `board query`."""
    rows = board.rows(LISTING)
    width = 1 + len(repos.COLUMNS)
    found = repos.parsed(board.plugin, [row[1:width] for row in rows if row[0] == "repo"])
    return found, [row[1 : 1 + len(TASK_COLUMNS)] for row in rows if row[0] == "task"]


def resolve(board: Board, tasks: list[Task]) -> Links:
    """What each task with an `after` waits on and each task's parent, by the task's path, and those not followed.

    One `board refs` answers every task with an `after` or a `parent`, in every status. An `after` or a
    `parent` that reaches a task in another notes repo than its own, such as a task in the board's own tree
    naming one in a mount, is listed and not followed.
    """
    asked = [task for task in tasks if task.waits or task.linked]
    links = Links({}, {}, [])
    for task, report in zip(asked, board.refs([task.path for task in asked], fields=FIELDS), strict=True):
        found, parent, across = blockers(report or {}, task.repo)
        if task.waits:
            links.blocked[task.path] = found
        if parent is not None:
            links.parents[task.path] = parent
        links.crossing.extend({"path": task.path, field: target} for field, target in across)
    return links


def blockers(report: dict[str, Any], repo: str) -> tuple[list[str], str | None, list[tuple[str, str]]]:
    """The tasks a task in `repo` reaches through `after` that are not done, its parent, and the targets elsewhere.

    Each blocker is listed once, by its path in the board, and an unresolved one as written; so is the
    parent. A target is done when it is a `tasks/task/v2` record whose status is `done` and which check
    degrades for nothing but the tree. A target held by another commit than the task, or the board's own
    tree for a mount, is in another repo, and is listed with its field, `after` or `parent`. Board displays
    a target in a mount by its path in that mount, so the repo's root is put in front.
    """
    own: dict[str, Any] = report.get("location") or {}
    source = own.get("source")
    found: list[str] = []
    parent: str | None = None
    across: list[tuple[str, str]] = []
    for outgoing in report.get("from", []):
        pointer = str(outgoing.get("pointer", ""))
        field = "after" if pointer.startswith("/after/") else "parent" if pointer == "/parent" else None
        if outgoing.get("link") or field is None:
            continue
        location = outgoing["location"]
        if location is not None and location.get("source") != source:
            across.append((field, str(outgoing["target"])))
            continue
        display = str(outgoing["target"] if location is None else location["display"])
        display = display if repo == "/" else repo + display
        if field == "parent":
            parent = display
            continue
        values = {value["pointer"]: value.get("value") for value in outgoing.get("values", [])}
        code = outgoing.get("target_code")
        done = values.get("/$type") == TASK_TYPE and values.get("/status") == "done" and code in (None, *TREE_CODES)
        if not done and display not in found:
            found.append(display)
    return found, parent, across


def grown(tasks: list[Task], links: Links) -> tuple[dict[str, Node], Forest]:
    """The tasks as the tree's nodes, by path, and the forest their parents make."""
    nodes = {task.path: Node(task.path, task.title, task.status, links.parents.get(task.path)) for task in tasks}
    return nodes, forest(nodes)


def waited(links: Links, tree: Forest, nodes: dict[str, Node], path: str) -> list[str]:
    """What a task waits on: its open `after` targets, then the open descendants nearest to it."""
    after = links.blocked.get(path, [])
    return after + [child for child in waits_on(tree, nodes, path) if child not in after]


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
    """The tasks in `roots`, in every status, and how many of their files are degraded or hold a value it cannot read.

    A task file is degraded whatever its status when it is no `tasks/task/v2` record, when check degrades it for
    more than the tree, or when the plugin's model refuses a record the board's schema admitted.
    """
    tasks: list[Task] = []
    degraded = 0
    for root, path, code, body in rows:
        at = repos.root(str(root))
        if at not in roots:
            continue
        if body is None or (code is not None and code not in TREE_CODES):
            degraded += 1
            continue
        try:
            written: dict[str, Any] = json.loads(body)
            task = models.Task.model_validate(written)
            defer = written.get("defer")
            if task.status == "open" and defer is not None and defer not in HORIZONS:
                until(str(defer))
        except ValueError:
            degraded += 1
            continue
        tasks.append(
            Task(
                at,
                str(path),
                task.title,
                task.due,
                None if defer is None else str(defer),
                task.from_hour,
                task.reviewed,
                list(task.contexts),
                bool(task.after),
                task.owner,
                task.status,
                task.parent is not None,
                task.kind or "task",
                task.brief if task.brief is None or at == "/" else at + task.brief,
            )
        )
    return tasks, degraded


def sweep(  # noqa: PLR0913 - the tasks, what they wait on, the clock, the owner and the tree
    tasks: list[Task],
    blocked: dict[str, list[str]],
    now: dt.datetime,
    start: dt.timedelta,
    owner: str | None = None,
    *,
    places: dict[str, dict[str, Any]] | None = None,
) -> dict[str, dict[str, Any]]:
    """The sections of the sweep of open `tasks` at `now`, for a day that starts `start` after midnight.

    It does no I/O. With an `owner`, every section keeps the tasks that owner or nobody holds, and `held`
    lists the ones that owner holds that are not available. `waiting` still counts every task. `places` holds
    each task's place in the tree; a task without one is a root with no subtasks.
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
            **(places or {}).get(task.path, place(forest({}), {}, task.path)),
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
