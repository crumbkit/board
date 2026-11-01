"""The write calls: `add`, `claim`, `done`, `defer`, `drop`, `reviewed` and `move` (README.md, Writing tasks).

Each call syncs a mounted repo before it writes and pushes after, as notes `add` does, and each but `move`
sets `reviewed` to the person's today.
"""

import datetime as dt
import json
from collections.abc import Callable, Mapping
from typing import Any

from board_plugin import repos, writing
from board_plugin.board import Board
from board_plugin.day import day_start, today
from board_plugin.protocol import CallRequest, PluginError
from board_plugin.repos import Repo
from board_plugin.slug import slug
from pydantic import ValidationError

from board_tasks import available
from board_tasks.available import HORIZONS
from board_tasks.lines import with_fields
from board_tasks.models import TASK_TYPE, Task
from board_tasks.repeat import completed
from board_tasks.tree import descendants

PLUGIN = "tasks"
# The refusal of a write that found the task busy: edited in its clone and not committed, or changing.
BUSY = "tasks::task::busy"
# How many names a new task tries, `<slug>.md`, then `<slug>-2.md` and on.
NAMES = 10
# The fields `add` writes in this order, after `$type`, `title` and `status`.
ADDED = ("kind", "brief", "parent", "due", "defer", "after", "repeat", "ephemeral", "from_hour", "contexts")
# What a call changes, from the task's record and the person's today.
Change = Callable[[dict[str, Any], dt.date], Mapping[str, str | None]]


def add(request: CallRequest) -> dict[str, object]:
    """Write a new open task to `tasks/<slug>.md` in the repo the caller's remote routes to."""
    board = Board(request.board.root, PLUGIN)
    given: dict[str, Any] = request.input
    repo = routed(board, given.get("repo"), given.get("remote") or origin(given.get("repo")))
    day = today(dt.datetime.now().astimezone(), day_start())
    fields: dict[str, Any] = {"$type": TASK_TYPE, "title": given["title"], "status": "open"}
    fields |= {key: given[key] for key in ADDED if given.get(key) is not None}
    if "defer" in fields:
        fields["defer"] = until(str(fields["defer"]), day)
    fields["reviewed"] = day.isoformat()
    text = (
        "---\n"
        + "".join(f"{key}: {json.dumps(value, ensure_ascii=False)}\n" for key, value in fields.items())
        + "---\n"
    )
    body = given.get("text")
    text += "" if body is None else "\n" + str(body).rstrip() + "\n"
    synced = writing.push(repo)
    path, commit = writing.create(repo, f"tasks/{slug(str(given['title']))}", text, names=NAMES, busy=BUSY)
    return written(repo, path, fields, commit, synced)


def origin(name: object) -> str | None:
    """The caller's origin to route a new task by, unless `--repo` names its repo; refused when git gave none.

    A caller inside a git repository whose origin git did not give may be in a work repository, so the task is
    not written to the personal repo.
    """
    if name is not None:
        return None
    remote, unread = repos.caller_origin()
    if unread is not None:
        why = f"{unread}, so the repo the task belongs to is not known; name it with --remote or --repo"
        raise PluginError("tasks::repo::origin_unread", why, {"origin_unread": unread})
    return remote


def claim(request: CallRequest) -> dict[str, object]:
    """Set the task's owner, refused when another owner holds it."""
    owner = str(request.input["owner"])

    def change(record: dict[str, Any], _: dt.date) -> dict[str, str]:
        held = record.get("owner")
        if held not in (None, owner):
            path = str(request.input["path"])
            why = f"{path} is held by {held}, so it is left as it is; leave it to {held}, or claim another task"
            raise PluginError("tasks::task::held", why, {"path": path, "owner": held})
        return {"owner": owner}

    return edit(request, change)


def done(request: CallRequest) -> dict[str, object]:
    """Mark the task done, or defer a repeating one to its next occurrence."""
    return edit(request, completed)


def defer(request: CallRequest) -> dict[str, object]:
    """Defer the task until a date, writing a horizon as the date it ends on."""
    return edit(request, lambda _, day: {"defer": until(str(request.input["until"]), day)})


def drop(request: CallRequest) -> dict[str, object]:
    """Mark the task dropped."""
    return edit(request, lambda _, __: {"status": "dropped"})


def reviewed(request: CallRequest) -> dict[str, object]:
    """Mark the task reviewed today, whatever its status."""
    return edit(request, lambda _, __: {}, finished=True)


def move(request: CallRequest) -> dict[str, object]:
    """Set the task's parent to another task in its repo, or clear it with `none`, and change nothing else.

    The new parent is refused when it is in another repo, holds no task, or is the task or one of its
    descendants, which would make a loop. The tree is read after the sync, on every attempt.
    """
    board = Board(request.board.root, PLUGIN)
    logical, parent = str(request.input["path"]), str(request.input["parent"])
    repo = holder(board, logical)
    fields: dict[str, object] = {"path": logical, "parent": parent}
    if parent != "none" and holder(board, parent).root != repo.root:
        why = (
            f"{parent} is in another repo than {logical}, and a parent is in its task's own repo; name a task in "
            f"{repo.root}"
        )
        raise PluginError("tasks::move::cross_repo", why, fields)

    def change(_record: dict[str, Any], _day: dt.date) -> dict[str, str | None]:
        if parent == "none":
            return {"parent": None}
        tasks, _ = available.read(available.listing(board)[1], [repo.root])
        _, tree = available.grown(tasks, available.resolve(board, tasks))
        if parent not in {task.path for task in tasks}:
            why = f"no task is at {parent}; give the path of a task, as `board call tasks.tree --root true` lists them"
            raise PluginError("tasks::move::parent_not_found", why, fields)
        if parent == logical or parent in descendants(tree, logical):
            why = f"{parent} is {logical} or under it, so the move would make a loop; give a task outside its tree"
            raise PluginError("tasks::move::cycle", why, fields)
        return {"parent": parent.removeprefix(repo.root.rstrip("/"))}

    output = edit(request, change, finished=True, stamped=False)
    keys = ("repo", "path", "commit", "unsynced", "unpushed")
    return {key: output[key] for key in keys} | {"parent": None if parent == "none" else parent}


def edit(request: CallRequest, change: Change, *, finished: bool = False, stamped: bool = True) -> dict[str, object]:
    """Set the fields `change` names on the task at the input's path, an open one unless `finished`.

    Unless `stamped` is False, it sets `reviewed` to today too.

    A write refused because the task or the clone moved since it was read is tried again on a new read. A new
    read that finds the text the last write sent, which Board wrote before it refused, is that write.
    """
    board = Board(request.board.root, PLUGIN)
    logical = str(request.input["path"])
    repo = holder(board, logical)
    path = logical.removeprefix(repo.root.rstrip("/") + "/")
    synced = writing.push(repo)
    day = today(dt.datetime.now().astimezone(), day_start())
    moved = sent = None
    for _ in range(writing.ATTEMPTS):
        repo.pin()
        task = repo.read(path)
        if sent is not None and task.text == sent and task.record is not None:
            return written(repo, path, task.record, None, synced)
        record = checked(task, repo, logical, finished=finished)
        values = {**change(record, day), **({"reviewed": day.isoformat()} if stamped else {})}
        sent = with_fields(str(task.text), values, logical)
        try:
            commit = repo.write(path, sent, task.blob)
        except PluginError as error:
            moved = writing.refused(error)
            if moved == writing.EDITED:
                why = (
                    f"{logical} changed in {repo.clone} after it was read; call again, and if this persists, commit "
                    "or discard the edit not committed there"
                )
                raise PluginError(BUSY, why, {"path": logical, "code": moved}) from error
            if moved not in writing.MOVED:
                raise
            continue
        return written(repo, path, record | values, commit, synced)
    why = (
        f"{logical} kept changing while it was written, {writing.ATTEMPTS} times; read it with `board get "
        f"{logical}`, and call again if it lacks the change"
    )
    raise PluginError(BUSY, why, {"path": logical, "code": moved})


def checked(task: repos.File, repo: Repo, logical: str, *, finished: bool) -> dict[str, Any]:
    """The task's record, refused unless it is a valid v2 task under `tasks/` in a notes repo, open unless `finished`.

    The file is in the notes repo `holder` chose only when `board get` read it from that repo's clone, or from
    the board's own tree for `/`. A task in a mount that is not a notes repo fails that.
    """
    if task.blob is None:
        why = f"no file is at {logical}; list the tasks with `board call tasks.available`"
        raise PluginError("tasks::task::not_found", why, {"path": logical})
    location = task.location or {}
    held = location.get("source") == "worktree" if repo.clone is None else location.get("repository") == repo.clone
    if not held:
        why = (
            f"{logical} is not in a notes repo, the board's own tree or a mount whose tree has daily/, so no "
            "call writes it; give a path under one, as `board call tasks.available` lists them"
        )
        raise PluginError("tasks::repo::not_found", why, {"path": logical})
    hint = f"run `board check {logical}` to see what it holds"
    if task.text is None or "/tasks/" not in logical.removeprefix(repo.root.rstrip("/")):
        why = f"{logical} is not a {TASK_TYPE} task under a tasks/ directory; {hint}"
        raise PluginError("tasks::task::not_task", why, {"path": logical})
    try:
        Task.model_validate(task.record or {})
    except ValidationError as error:
        why = f"{logical} is not a valid {TASK_TYPE} task ({error.error_count()} errors); {hint}"
        raise PluginError("tasks::task::not_task", why, {"path": logical}) from error
    record = task.record or {}
    status = record.get("status")
    if not finished and status != "open":
        why = f"{logical} is {status}, so it is left as it is; set `status: open` in it to reopen it first"
        raise PluginError("tasks::task::finished", why, {"path": logical, "status": status})
    return record


def routed(board: Board, name: object, remote: object) -> Repo:
    """The repo a new task goes to: the one named; else the one covering the remote; else the personal one; else `/`.

    Two repos covering the remote, or two personal ones, are refused, since a task must not cross a silo.
    """
    found = repos.listed(board)
    clones = {at: clone for at, _, clone, _ in found} | {"/": None}
    if name is not None:
        at = repos.root(str(name))
        if at not in clones:
            why = f"no repo is at {name} (repos: {', '.join(sorted(clones))}); name one listed with --repo"
            raise PluginError("tasks::repo::not_found", why, {"repos": sorted(clones)})
        return Repo(board, at, clones[at])
    covering, personal = repos.covering(PLUGIN, found, None if remote is None else str(remote))
    candidates = covering or personal
    if len(candidates) > 1:
        why = f"{', '.join(candidates)} could each hold the task, so none was written; name one with --repo"
        raise PluginError("tasks::repo::ambiguous", why, {"candidates": candidates})
    at = candidates[0] if candidates else "/"
    return Repo(board, at, clones[at])


def holder(board: Board, logical: str) -> Repo:
    """The repo that holds the board path `logical`: the mount whose root is the longest prefix of it, else `/`."""
    found = [(at, clone) for at, _, clone, _ in repos.listed(board) if logical.startswith(at.rstrip("/") + "/")]
    at, clone = max(found, key=lambda each: len(each[0]), default=("/", None))
    return Repo(board, at, clone)


def until(value: str, day: dt.date) -> str:
    """The date a `defer` ends: a date as it is, or a horizon counted from `day`."""
    return (day + dt.timedelta(days=HORIZONS[value])).isoformat() if value in HORIZONS else value


def written(repo: Repo, path: str, record: dict[str, Any], commit: str | None, synced: str | None) -> dict[str, object]:
    """The call's output, once the write is made: the clone is pushed, and the mount's pin moved after it."""
    commit, unpushed = writing.landed(repo, commit)
    return {
        "repo": repo.root,
        "path": repo.logical(path),
        "status": record["status"],
        "defer": None if record.get("defer") is None else str(record["defer"]),
        "owner": record.get("owner"),
        "reviewed": record.get("reviewed"),
        "commit": commit,
        "unsynced": synced,
        "unpushed": unpushed,
    }
