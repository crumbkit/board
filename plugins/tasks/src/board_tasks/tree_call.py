"""The `tree` capability: one task and its descendants, or every top-level tree in scope (README.md)."""

from typing import Any

from board_plugin import repos
from board_plugin.board import Board
from board_plugin.protocol import CallRequest, PluginError

from board_tasks.available import SHOWN, Task, grown, held_by, listing, read, resolve, waited
from board_tasks.tree import subtasks, tree_path


def tree(request: CallRequest) -> dict[str, object]:
    """List the tree of `--path`, or with `--root` every top-level node's, read at the repos' pins."""
    board = Board(request.board.root, "tasks")
    given: dict[str, Any] = request.input
    path, root = given.get("path"), given.get("root") is True
    if (path is None) == (not root):
        why = "give either --path <task> for one task's tree or --root true for every top-level tree, not both"
        raise PluginError("tasks::tree::path_or_root", why, {"path": path, "root": given.get("root")})
    found, rows = listing(board)
    if path is not None:
        roots = [held_by(found, str(path))]
    else:
        name, remote = given.get("repo"), given.get("remote")
        unread = None
        if name is None and remote is None:
            remote, unread = repos.caller_origin()
        roots = repos.scope(board.plugin, found, None if name is None else str(name), remote, unread)
    tasks, _ = read(rows, roots)
    links = resolve(board, tasks)
    nodes, forest = grown(tasks, links)
    by_path = {task.path: task for task in tasks}
    if path is not None and path not in by_path:
        why = f"no task is at {path}; list the trees with `board call tasks.tree --root true`"
        raise PluginError("tasks::task::not_found", why, {"path": path})
    shown = [] if path is not None else sorted(forest.roots, key=lambda each: opened_first(by_path[each]))

    def node(at: str) -> dict[str, Any]:
        """The node at `at`, with its children by title."""
        task = by_path[at]
        return {
            "path": at,
            "title": task.title,
            "status": task.status,
            "kind": task.kind,
            "brief": task.brief,
            "owner": task.owner,
            "blocked_by": waited(links, forest, nodes, at),
            "subtasks": subtasks(forest, nodes, at),
            "tree_path": tree_path(forest, at),
            "children": [node(child) for child in forest.children.get(at, [])],
        }

    return {
        "tree": None if path is None else node(str(path)),
        "roots": [node(each) for each in shown[:SHOWN]],
        "total": len(shown),
        "repos": roots,
        "cycles": forest.cycles,
        "cross_repo": links.crossing,
    }


def opened_first(task: Task) -> tuple[bool, str, str]:
    """The order of top-level nodes: open ones first, then by title and path."""
    return task.status != "open", task.title, task.path
