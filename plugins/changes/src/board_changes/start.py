"""The `start` capability: a new change in `plan`, its plan to fill in, its route and the repository's brief."""

import re

from board_plugin.board import Board
from board_plugin.protocol import CallRequest, PluginError

from board_changes.change import CHANGE_TYPE, PLUGIN, Change, write
from board_changes.gates import SECTIONS
from board_changes.repo import CODEOWNERS, DOCS, ask_first, exists, files, owners, route, suite_in

# The longest name a title is shortened to.
NAME_MAX = 60


def name_of(title: str) -> str:
    """A change's name from its title: lowercase words joined by hyphens."""
    return re.sub(r"[^a-z0-9]+", "-", title.casefold()).strip("-")[:NAME_MAX].strip("-")


def start(request: CallRequest) -> dict[str, object]:
    """Create the change at `/changes/<name>` and its plan, and return the route and the brief."""
    asked = request.input
    repo, title = str(asked["repo"]).rstrip("/"), str(asked["title"])
    name = str(asked.get("name") or name_of(title))
    handle = asked.get("handle")
    path, plan = f"/changes/{name}", f"/changes/{name}/plan.md"
    board = Board(request.board.root, PLUGIN)
    exists(board, repo)
    held = board.get([path, plan])
    if not isinstance(held[0], PluginError):
        raise PluginError(
            "changes::change::exists",
            f"a change is already at {path}; give another --name, or resume it with "
            f"`board call changes.next --path {path}`",
            {"path": path},
        )
    codeowners = files(board, repo, CODEOWNERS)
    rules = owners(next(iter(codeowners.values()), ""))
    asking = ask_first(rules, handle if isinstance(handle, str) else None)
    docs = files(board, repo, DOCS)
    change: Change = {"$type": CHANGE_TYPE, "title": title, "repo": repo, "phase": "plan", "plan": plan}
    change["route"] = route(rules, asking)
    lines: list[dict[str, object]] = [{"path": path, "expect": "none", "record": change}]
    if isinstance(held[1], PluginError):
        skeleton = f"# {title}\n" + "".join(f"\n## {section}\n" for section in SECTIONS)
        lines.append({"path": plan, "expect": "none", "text": skeleton})
    write(board, lines)
    return {
        "path": path,
        "plan": plan,
        "route": change["route"],
        "owners": rules,
        "ask_first": asking,
        "codeowners": next(iter(codeowners), None),
        "docs": list(docs),
        "suite": suite_in(board, repo, docs),
    }
