"""Projects and tasks as one tree through the real `board`: `parent` in `available`, and `tree`.

test_tree.py holds the rules of the tree itself.
"""

import json
from typing import Any

import pytest
from conftest import Board, notes_repo, task_v2

# Ship flows editor (project)
# ├── Dispatch owns flow documents
# │   └── Flow document store (project)
# │       ├── Choose store
# │       └── Migrate existing flows (done)
# ├── Flows UX agreed
# └── Pin lines to revisions (done)
FLOWS = {
    "ship-flows-editor": task_v2("Ship flows editor", kind="project", brief="/projects/flows/README.md"),
    "flows-ux-agreed": task_v2("Flows UX agreed", parent="/tasks/ship-flows-editor.md"),
    "dispatch-owns-flow-documents": task_v2(
        "Dispatch owns flow documents", parent="/tasks/ship-flows-editor.md", owner="lead"
    ),
    "flow-document-store": task_v2(
        "Flow document store", kind="project", parent="/tasks/dispatch-owns-flow-documents.md"
    ),
    "choose-store": task_v2("Choose store", parent="/tasks/flow-document-store.md"),
    "migrate-existing-flows": task_v2("Migrate existing flows", "done", parent="/tasks/flow-document-store.md"),
    "pin-lines-to-revisions": task_v2("Pin lines to revisions", "done", parent="/tasks/ship-flows-editor.md"),
}


def call(board: Board, capability: str, *flags: str) -> tuple[int, dict[str, Any]]:
    """`board --json call tasks.<capability> <flags>` from outside any git repository: the exit code and output."""
    outside = board.root.parent / "outside"
    outside.mkdir(exist_ok=True)
    ran = board.run("--json", "call", f"tasks.{capability}", *flags, cwd=outside)
    document: dict[str, Any] = json.loads(ran.stdout)
    return ran.returncode, document["data"]["output"] if ran.returncode == 0 else document


@pytest.fixture
def flows(board: Board) -> Board:
    """The board holding the flows tree in its own tree, with the project's brief."""
    board.write("projects/flows/README.md", "# Flows\n")
    for name, text in FLOWS.items():
        board.write(f"tasks/{name}.md", text)
    return board


def listed(output: dict[str, Any]) -> dict[str, dict[str, Any]]:
    """Every task `available` lists, by title. No fixture task has `reviewed`, so each one not available is stale."""
    return {item["title"]: item for section in ("available", "stale") for item in output[section]["tasks"]}


def test_a_parent_waits_on_its_open_subtasks_until_each_is_finished(flows: Board) -> None:
    code, output = call(flows, "available")
    assert code == 0, output
    assert [item["title"] for item in output["available"]["tasks"]] == ["Choose store", "Flows UX agreed"]
    shown = listed(output)
    assert {title: item["blocked_by"] for title, item in shown.items() if item["blocked_by"]} == {
        "Dispatch owns flow documents": ["/tasks/flow-document-store.md"],
        "Flow document store": ["/tasks/choose-store.md"],
        "Ship flows editor": ["/tasks/dispatch-owns-flow-documents.md", "/tasks/flows-ux-agreed.md"],
    }
    dispatch = shown["Dispatch owns flow documents"]
    assert (dispatch["parent"], dispatch["subtasks"], dispatch["tree_path"]) == (
        "/tasks/ship-flows-editor.md",
        {"open": 2, "total": 3},
        "ship-flows-editor/dispatch-owns-flow-documents",
    )
    ship = shown["Ship flows editor"]
    assert (ship["parent"], ship["subtasks"], ship["tree_path"]) == (None, {"open": 4, "total": 6}, "ship-flows-editor")
    assert shown["Choose store"]["waiting"] == 1
    flows.write("tasks/choose-store.md", task_v2("Choose store", "done", parent="/tasks/flow-document-store.md"))
    assert [item["title"] for item in call(flows, "available")[1]["available"]["tasks"]] == [
        "Flow document store",
        "Flows UX agreed",
    ]


def test_a_parent_marked_done_early_still_holds_its_open_subtasks_back_from_its_own_parent(flows: Board) -> None:
    store = task_v2("Flow document store", "done", parent="/tasks/dispatch-owns-flow-documents.md")
    flows.write("tasks/flow-document-store.md", store)
    shown = listed(call(flows, "available")[1])
    assert shown["Dispatch owns flow documents"]["blocked_by"] == ["/tasks/choose-store.md"]


NODE: dict[str, Any] = {"brief": None, "kind": "task", "owner": None, "blocked_by": [], "children": []}


def test_tree_returns_the_task_and_its_descendants_in_every_status_nested_by_title(flows: Board) -> None:
    code, output = call(flows, "tree", "--path", "/tasks/dispatch-owns-flow-documents.md")
    assert code == 0, output
    store = "ship-flows-editor/dispatch-owns-flow-documents/flow-document-store"
    assert output == {
        "tree": NODE
        | {
            "path": "/tasks/dispatch-owns-flow-documents.md",
            "title": "Dispatch owns flow documents",
            "status": "open",
            "owner": "lead",
            "tree_path": "ship-flows-editor/dispatch-owns-flow-documents",
            "blocked_by": ["/tasks/flow-document-store.md"],
            "subtasks": {"open": 2, "total": 3},
            "children": [
                NODE
                | {
                    "path": "/tasks/flow-document-store.md",
                    "title": "Flow document store",
                    "status": "open",
                    "kind": "project",
                    "tree_path": store,
                    "blocked_by": ["/tasks/choose-store.md"],
                    "subtasks": {"open": 1, "total": 2},
                    "children": [
                        NODE
                        | {
                            "path": "/tasks/choose-store.md",
                            "title": "Choose store",
                            "status": "open",
                            "tree_path": f"{store}/choose-store",
                            "subtasks": {"open": 0, "total": 0},
                        },
                        NODE
                        | {
                            "path": "/tasks/migrate-existing-flows.md",
                            "title": "Migrate existing flows",
                            "status": "done",
                            "tree_path": f"{store}/migrate-existing-flows",
                            "subtasks": {"open": 0, "total": 0},
                        },
                    ],
                }
            ],
        },
        "roots": [],
        "total": 0,
        "repos": ["/"],
        "cycles": [],
        "cross_repo": [],
    }


def test_tree_root_lists_every_top_level_node_open_ones_first_with_its_descendants(flows: Board) -> None:
    flows.write("tasks/alone.md", task_v2("Alone"))
    flows.write("tasks/old.md", task_v2("Old", "done"))
    code, output = call(flows, "tree", "--root", "true")
    assert code == 0, output
    assert [(root["title"], len(root["children"])) for root in output["roots"]] == [
        ("Alone", 0),
        ("Ship flows editor", 3),
        ("Old", 0),
    ]
    [_, ship, _] = output["roots"]
    assert (ship["kind"], ship["brief"]) == ("project", "/projects/flows/README.md")
    assert (output["tree"], output["total"], output["repos"]) == (None, 3, ["/"])


def test_a_title_change_keeps_the_tasks_path_and_its_tree_path(flows: Board) -> None:
    code, added = call(flows, "add", "--title", "Write the brief", "--parent", "/tasks/ship-flows-editor.md")
    assert (code, added["path"]) == (0, "/tasks/write-the-brief.md"), added
    text = (flows.root / "tasks" / "write-the-brief.md").read_text()
    flows.write("tasks/write-the-brief.md", text.replace('"Write the brief"', '"Draft the flows brief"'))
    [item] = [item for item in call(flows, "available")[1]["available"]["tasks"] if item["title"].startswith("Draft")]
    assert (item["path"], item["tree_path"]) == ("/tasks/write-the-brief.md", "ship-flows-editor/write-the-brief")


def test_tree_refuses_a_path_that_holds_no_task_and_admits_a_task_beside_it(flows: Board) -> None:
    flows.write("tasks/note.md", "# A note, not a task\n")
    for path in ("/tasks/gone.md", "/tasks/note.md"):
        code, refused = call(flows, "tree", "--path", path)
        assert (code, refused["code"], refused["fields"]["path"]) == (1, "tasks::task::not_found", path), refused
    assert call(flows, "tree", "--path", "/tasks/choose-store.md")[0] == 0


@pytest.mark.parametrize(
    ("flags", "admitted"),
    [
        ((), False),
        (("--path", "/tasks/alone.md", "--root", "true"), False),
        (("--root", "false"), False),
        (("--path", "/tasks/alone.md"), True),
        (("--root", "true"), True),
    ],
)
def test_tree_takes_either_a_path_or_root_and_refuses_both_or_neither(
    board: Board,
    flags: tuple[str, ...],
    admitted: bool,  # noqa: FBT001 - pytest passes the case's flag
) -> None:
    board.write("tasks/alone.md", task_v2("Alone"))
    code, output = call(board, "tree", *flags)
    assert (code == 0, None if admitted else output["code"]) == (
        admitted,
        None if admitted else "tasks::tree::path_or_root",
    )


def test_a_parent_in_another_repo_is_listed_and_not_followed_and_one_in_its_own_repo_is(board: Board) -> None:
    notes_repo(board, "work", {"covers": []}, {"plan": task_v2("Work plan")})
    board.write("tasks/plan.md", task_v2("Plan"))
    board.write("tasks/crossing.md", task_v2("Crossing", parent="/repos/work/tasks/plan.md"))
    board.write("tasks/inside.md", task_v2("Inside", parent="/tasks/plan.md"))
    code, output = call(board, "available")
    assert code == 0, output
    assert output["cross_repo"] == [{"path": "/tasks/crossing.md", "parent": "/repos/work/tasks/plan.md"}]
    shown = listed(output)
    assert (shown["Crossing"]["parent"], shown["Crossing"]["tree_path"]) == (None, "crossing")
    assert (shown["Inside"]["parent"], shown["Inside"]["tree_path"]) == ("/tasks/plan.md", "plan/inside")
    code, tree = call(board, "tree", "--root", "true", "--repo", "/")
    assert (code, tree["cross_repo"]) == (0, output["cross_repo"]), tree


def test_a_parent_in_a_mount_resolves_in_that_mount(board: Board) -> None:
    tasks = {"plan": task_v2("Plan"), "piece": task_v2("Piece", parent="/tasks/plan.md")}
    notes_repo(board, "personal", {"covers": [], "personal": True}, tasks)
    board.write("tasks/plan.md", task_v2("The board's plan", "done"))
    shown = listed(call(board, "available", "--repo", "/repos/personal")[1])
    assert shown["Plan"]["blocked_by"] == ["/repos/personal/tasks/piece.md"]
    assert (shown["Piece"]["parent"], shown["Piece"]["tree_path"]) == ("/repos/personal/tasks/plan.md", "plan/piece")
    code, output = call(board, "tree", "--path", "/repos/personal/tasks/plan.md")
    assert (code, [child["path"] for child in output["tree"]["children"]]) == (0, ["/repos/personal/tasks/piece.md"])
    assert output["repos"] == ["/repos/personal"]


def test_a_parent_loop_is_cut_at_its_first_path_and_listed_in_cycles(board: Board) -> None:
    board.write("tasks/a.md", task_v2("A", parent="/tasks/b.md"))
    board.write("tasks/b.md", task_v2("B", parent="/tasks/a.md"))
    code, output = call(board, "tree", "--root", "true")
    assert code == 0, output
    assert ([root["path"] for root in output["roots"]], output["cycles"]) == (["/tasks/a.md"], ["/tasks/a.md"])
    assert [item["title"] for item in call(board, "available")[1]["available"]["tasks"]] == ["B"]


def test_a_tree_with_no_parent_reads_the_tasks_with_one_query_and_no_refs(board: Board) -> None:
    board.write("tasks/alone.md", task_v2("Alone"))
    log = board.cache / "board" / "timings.jsonl"
    before = len(log.read_text().splitlines()) if log.exists() else 0
    assert call(board, "tree", "--path", "/tasks/alone.md")[0] == 0
    nested = [json.loads(line) for line in log.read_text().splitlines()[before:]]
    assert [record["command"] for record in nested if record["depth"] > 0] == ["query"]
