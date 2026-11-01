"""The `tree` capability: one task and its descendants, or every top-level tree in scope (README.md)."""

from typing import Any

from board_plugin import repos
from board_plugin.board import Board
from board_plugin.protocol import CallRequest, PluginError

from board_tasks.available import SHOWN, Links, Task, grown, listing, read, resolve, waited
from board_tasks.tree import Forest, Node, subtasks, tree_path


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
        roots = [holder(found, str(path))]
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
    grow = Grower(by_path, nodes, forest, links)
    return {
        "tree": None if path is None else grow.node(str(path)),
        "roots": [grow.node(each) for each in shown[:SHOWN]],
        "total": len(shown),
        "repos": roots,
        "cycles": forest.cycles,
        "cross_repo": links.crossing,
    }


def holder(found: list[repos.Listed], path: str) -> str:
    """The root of the repo that holds `path`: the mount whose root is the longest prefix of it, else `/`."""
    held = [at for at, _, _, _ in found if path.startswith(at.rstrip("/") + "/")]
    return max(held, key=len, default="/")


def opened_first(task: Task) -> tuple[bool, str, str]:
    """The order of top-level nodes: open ones first, then by title and path."""
    return task.status != "open", task.title, task.path


class Grower:
    """Builds the nested nodes of `tree`'s output from the tasks and the forest."""

    def __init__(self, tasks: dict[str, Task], nodes: dict[str, Node], forest: Forest, links: Links) -> None:
        """Hold what each node is built from."""
        self.tasks, self.nodes, self.forest, self.links = tasks, nodes, forest, links

    def node(self, path: str) -> dict[str, Any]:
        """The node at `path`, with its children by title."""
        task = self.tasks[path]
        return {
            "path": path,
            "title": task.title,
            "status": task.status,
            "kind": task.kind,
            "brief": task.brief,
            "owner": task.owner,
            "blocked_by": waited(self.links, self.forest, self.nodes, path),
            "subtasks": subtasks(self.forest, self.nodes, path),
            "tree_path": tree_path(self.forest, path),
            "children": [self.node(child) for child in self.forest.children.get(path, [])],
        }
