"""The `start` capability: a new change in `plan`, its plan to fill in, its route and the repository's brief."""

import re
from collections.abc import Mapping

from board_plugin.board import Board
from board_plugin.protocol import CallRequest, PluginError

from board_changes.change import CHANGE_TYPE, PLUGIN, TASK_TYPE, Change, write
from board_changes.gates import SECTIONS
from board_changes.phases import check_target
from board_changes.repo import CODEOWNERS, DOCS, ask_first, exists, files, owners, route, suite_in
from board_changes.tasks import add_task, routing

# The longest name a title is shortened to.
NAME_MAX = 60


def name_of(title: str) -> str:
    """A change's name from its title: lowercase words joined by hyphens."""
    return re.sub(r"[^a-z0-9]+", "-", title.casefold()).strip("-")[:NAME_MAX].strip("-")


def refuse_existing(board: Board, path: str) -> None:
    """Refuse a change path that already holds a change with `changes::change::exists`."""
    [held] = board.get([path])
    if not isinstance(held, PluginError):
        raise PluginError(
            "changes::change::exists",
            f"a change is already at {path}; give another --name, or resume it with "
            f"`board call changes.next --path {path}`",
            {"path": path},
        )


def task_for(board: Board, asked: Mapping[str, object], title: str) -> str:
    """The change's task: the one `task` names, checked to be a task, or a new one titled `title`."""
    named = asked.get("task")
    if isinstance(named, str):
        check_target(board, named, (TASK_TYPE,))
        return named
    repo = asked.get("task_repo")
    return add_task(board, title, routing(repo if isinstance(repo, str) else None))


def start(request: CallRequest) -> dict[str, object]:
    """Create the change at `/changes/<name>`, its plan and, unless one is named, its task, and return the brief."""
    asked = request.input
    mount, title = str(asked["mount"]).rstrip("/"), str(asked["title"])
    name = str(asked.get("name") or name_of(title))
    handle = asked.get("handle")
    path, plan = f"/changes/{name}", f"/changes/{name}/plan.md"
    board = Board(request.board.root, PLUGIN)
    exists(board, mount)
    refuse_existing(board, path)
    [held] = board.get([plan])
    codeowners = files(board, mount, CODEOWNERS)
    rules = owners(next(iter(codeowners.values()), ""))
    asking = ask_first(rules, handle if isinstance(handle, str) else None)
    docs = files(board, mount, DOCS)
    change: Change = {"$type": CHANGE_TYPE, "title": title, "mount": mount, "phase": "plan", "plan": plan}
    change["route"] = route(rules, asking)
    change["task"] = task_for(board, asked, title)
    lines: list[dict[str, object]] = [{"path": path, "expect": "none", "record": change}]
    if isinstance(held, PluginError):
        skeleton = f"# {title}\n" + "".join(f"\n## {section}\n" for section in SECTIONS)
        lines.append({"path": plan, "expect": "none", "text": skeleton})
    write(board, lines)
    return {
        "path": path,
        "plan": plan,
        "task": change["task"],
        "route": change["route"],
        "owners": rules,
        "ask_first": asking,
        "codeowners": next(iter(codeowners), None),
        "docs": list(docs),
        "suite": suite_in(board, mount, docs),
    }
