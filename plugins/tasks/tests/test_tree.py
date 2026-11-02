"""The tree that `parent` links make, from nodes as `available` and `tree` read them, with no `board` run.

These cases pin the rules a port of the plugin keeps: which tasks a parent waits on, what its subtasks count,
its tree path, and where a loop is cut.
"""

from board_tasks.tree import Node, descendants, forest, subtasks, tree_path, waits_on


def nodes(*given: tuple[str, str, str | None] | tuple[str, str, str | None, str]) -> dict[str, Node]:
    """Nodes by path from (slug, status, parent slug) and an optional title, which is the slug when left out."""
    found: dict[str, Node] = {}
    for item in given:
        name, status, parent = item[:3]
        title = item[3] if len(item) == 4 else name  # noqa: PLR2004 - a title is the fourth item
        path = f"/tasks/{name}.md"
        found[path] = Node(path, title, status, None if parent is None else f"/tasks/{parent}.md")
    return found


# Ship flows editor (project)
# ├── Flows UX agreed
# ├── Dispatch owns flow documents (task)
# │   └── Flow document store (project)
# │       ├── Choose store
# │       └── Migrate existing flows (done)
# └── Pin lines to revisions (done)
FLOWS = nodes(
    ("ship-flows-editor", "open", None, "Ship flows editor"),
    ("flows-ux-agreed", "open", "ship-flows-editor", "Flows UX agreed"),
    ("dispatch-owns-flow-documents", "open", "ship-flows-editor", "Dispatch owns flow documents"),
    ("flow-document-store", "open", "dispatch-owns-flow-documents", "Flow document store"),
    ("choose-store", "open", "flow-document-store", "Choose store"),
    ("migrate-existing-flows", "done", "flow-document-store", "Migrate existing flows"),
    ("pin-lines-to-revisions", "done", "ship-flows-editor", "Pin lines to revisions"),
)


def at(name: str) -> str:
    """The path of the fixture task named `name`."""
    return f"/tasks/{name}.md"


def test_a_project_under_a_task_puts_its_children_among_the_tasks_descendants() -> None:
    tree = forest(FLOWS)
    assert tree.roots == [at("ship-flows-editor")]
    assert descendants(tree, at("dispatch-owns-flow-documents")) == [
        at("flow-document-store"),
        at("choose-store"),
        at("migrate-existing-flows"),
    ]
    assert subtasks(tree, FLOWS, at("dispatch-owns-flow-documents")) == {"open": 2, "total": 3}
    assert subtasks(tree, FLOWS, at("ship-flows-editor")) == {"open": 4, "total": 6}
    assert subtasks(tree, FLOWS, at("choose-store")) == {"open": 0, "total": 0}


def test_children_are_sorted_by_title() -> None:
    tree = forest(FLOWS)
    assert tree.children[at("ship-flows-editor")] == [
        at("dispatch-owns-flow-documents"),
        at("flows-ux-agreed"),
        at("pin-lines-to-revisions"),
    ]


def test_the_tree_path_joins_the_slugs_from_the_root() -> None:
    tree = forest(FLOWS)
    assert (
        tree_path(tree, at("choose-store"))
        == "ship-flows-editor/dispatch-owns-flow-documents/flow-document-store/choose-store"
    )
    assert tree_path(tree, at("ship-flows-editor")) == "ship-flows-editor"


def test_a_parent_waits_on_its_open_children_and_on_the_open_descendants_of_a_finished_child() -> None:
    tree = forest(FLOWS)
    assert waits_on(tree, FLOWS, at("ship-flows-editor")) == [at("dispatch-owns-flow-documents"), at("flows-ux-agreed")]
    early = FLOWS | nodes(("flow-document-store", "done", "dispatch-owns-flow-documents", "Flow document store"))
    assert waits_on(forest(early), early, at("dispatch-owns-flow-documents")) == [at("choose-store")]
    finished = early | nodes(("choose-store", "dropped", "flow-document-store", "Choose store"))
    assert waits_on(forest(finished), finished, at("dispatch-owns-flow-documents")) == []


def test_a_parent_that_names_no_node_makes_a_root() -> None:
    given = nodes(("orphan", "open", "gone"), ("child", "open", "orphan"))
    tree = forest(given)
    assert (tree.roots, tree.cycles) == ([at("orphan")], [])
    assert tree_path(tree, at("child")) == "orphan/child"


def test_a_loop_is_cut_at_its_first_path_which_becomes_a_root_and_is_listed() -> None:
    given = nodes(("b", "open", "c"), ("c", "open", "a"), ("a", "open", "b"), ("d", "open", "a"))
    tree = forest(given)
    assert (tree.roots, tree.cycles) == ([at("a")], [at("a")])
    assert tree_path(tree, at("b")) == "a/c/b"
    assert descendants(tree, at("a")) == [at("c"), at("b"), at("d")]
    assert waits_on(tree, given, at("c")) == [at("b")]
    unlooped = given | nodes(("a", "open", None))
    assert (forest(unlooped).roots, forest(unlooped).cycles) == ([at("a")], [])


def test_a_task_that_is_its_own_parent_is_a_loop_of_one() -> None:
    given = nodes(("self", "open", "self"))
    tree = forest(given)
    assert (tree.roots, tree.cycles, descendants(tree, at("self"))) == ([at("self")], [at("self")], [])
