"""The tree that tasks' `parent` links make (README.md, Projects and trees). It does no I/O.

Each node has at most one parent, so the links make a forest once every loop is cut. A loop is cut at the
node with the first path in it, which then has no parent and is listed in `cycles`. A parent that names no
node, because it reaches nothing or another repo, leaves its node a root.
"""

from collections.abc import Mapping
from dataclasses import dataclass


@dataclass(frozen=True)
class Node:
    """A task in the tree: its board path, title, status, and its parent's board path as resolved, or None."""

    path: str
    title: str
    status: str
    parent: str | None


@dataclass(frozen=True)
class Forest:
    """The links once loops are cut: each node's parent, its children by title, the roots by path, and the cuts."""

    parents: dict[str, str]
    children: dict[str, list[str]]
    roots: list[str]
    cycles: list[str]


def forest(nodes: Mapping[str, Node]) -> Forest:
    """The forest the nodes' parents make, with each loop cut at its first path."""
    parents = {path: node.parent for path, node in nodes.items() if node.parent is not None and node.parent in nodes}
    cycles: list[str] = []
    settled: set[str] = set()
    for start in sorted(nodes):
        walk: list[str] = []
        at: str | None = start
        while at is not None and at not in settled and at not in walk:
            walk.append(at)
            at = parents.get(at)
        if at is not None and at in walk:
            cut = min(walk[walk.index(at) :])
            del parents[cut]
            cycles.append(cut)
        settled.update(walk)
    children: dict[str, list[str]] = {}
    for path in sorted(parents, key=lambda path: (nodes[path].title, path)):
        children.setdefault(parents[path], []).append(path)
    roots = sorted(path for path in nodes if path not in parents)
    return Forest(parents, children, roots, sorted(cycles))


def slug(path: str) -> str:
    """The slug of a task's path: its file name without `.md`."""
    return path.rsplit("/", 1)[-1].removesuffix(".md")


def tree_path(tree: Forest, path: str) -> str:
    """The slugs from the node's root to the node, joined by `/`."""
    chain = [path]
    while chain[-1] in tree.parents:
        chain.append(tree.parents[chain[-1]])
    return "/".join(slug(each) for each in reversed(chain))


def descendants(tree: Forest, path: str) -> list[str]:
    """Every node under `path`, depth first, each node's children by title."""
    found: list[str] = []
    for child in tree.children.get(path, []):
        found += [child, *descendants(tree, child)]
    return found


def subtasks(tree: Forest, nodes: Mapping[str, Node], path: str) -> dict[str, int]:
    """How many of the node's descendants are open, and how many there are."""
    under = descendants(tree, path)
    return {"open": sum(1 for each in under if nodes[each].status == "open"), "total": len(under)}


def waits_on(tree: Forest, nodes: Mapping[str, Node], path: str) -> list[str]:
    """The open descendants a node waits on: each open child, and under a finished child its own open ones."""
    found: list[str] = []
    for child in tree.children.get(path, []):
        found += [child] if nodes[child].status == "open" else waits_on(tree, nodes, child)
    return found


def place(tree: Forest, nodes: Mapping[str, Node], path: str) -> dict[str, object]:
    """The node's place in the tree, as `available` lists it: its parent, its subtasks and its tree path."""
    return {
        "parent": tree.parents.get(path),
        "subtasks": subtasks(tree, nodes, path),
        "tree_path": tree_path(tree, path),
    }
