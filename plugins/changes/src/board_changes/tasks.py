"""A change's task: one the caller names, or one this plugin asks the tasks plugin to add.

The task is written by `board call tasks.add`, so the tasks plugin's routing,
sync and push apply, and this plugin never writes a task file itself.
"""

import json

from board_plugin import repos
from board_plugin.board import Board
from board_plugin.protocol import PluginError

# Where a notes repo keeps its tasks, which splits a task's board path into its repo and its path in that repo.
TASKS_DIR = "/tasks/"


def split_task(path: str) -> tuple[str, str]:
    """A task's board path as its notes repo and its path in that repo.

    `/repos/notes/tasks/a.md` is (`/repos/notes`, `/tasks/a.md`), and
    `/tasks/a.md` is (`/`, `/tasks/a.md`).
    """
    at = path.rfind(TASKS_DIR)
    if at < 0:
        raise PluginError(
            "changes::task::not_a_task_path",
            f"{path} is not under a tasks/ directory, so its notes repo is not known; name a task such as "
            "/repos/notes-personal/tasks/<slug>.md, and list them with `board call tasks.available`",
            {"path": path},
        )
    return path[:at] or "/", path[at:]


def routing(repo: str | None) -> dict[str, str]:
    """The `tasks.add` input that sends a new task to `repo`, or else to the repo the caller's origin routes to.

    A nested `board call` runs in the board's root, so the caller's origin is
    passed on as `--remote`. A caller in a git repository whose origin git did
    not give is refused, as `tasks.add` refuses it.
    """
    if repo is not None:
        return {"repo": repo}
    remote, unread = repos.caller_origin()
    if unread is not None:
        raise PluginError(
            "changes::task::unrouted",
            f"{unread}, so the notes repo for the change's task is not known; name an existing task with --task, "
            "or the repo to add one to with --task-repo",
            {"origin_unread": unread},
        )
    return {} if remote is None else {"remote": remote}


def add_task(board: Board, title: str, route: dict[str, str], parent: str | None = None) -> str:
    """Add an open task titled `title` through `tasks.add`, sent where `route` says, and return its board path.

    `parent` is the parent task's board path; the new task goes to the
    parent's repo, since a task's parent is in its own repo.
    """
    given: dict[str, str] = {"title": title, **route}
    if parent is not None:
        given["repo"], given["parent"] = split_task(parent)
        given.pop("remote", None)
    report = board.document("call", "tasks.add", stdin=json.dumps(given).encode())
    return str(report["data"]["output"]["path"])
