"""The `prototype` and `verdict` capabilities: a change that answers one question, and what it found."""

from dataclasses import replace

from board_plugin.board import Board
from board_plugin.protocol import CallRequest, PluginError

from board_changes.change import CHANGE_TYPE, FINAL, PLUGIN, Change, branch_of, kind_of, read, write
from board_changes.plan import brief_of, plan_lines
from board_changes.repo import exists
from board_changes.start import name_of, refuse_existing
from board_changes.tasks import add_task, close_task
from board_changes.wip import refuse_full

# The phase a verdict ends a prototype in, by its state.
ENDS = {"settled": "done", "abandoned": "dropped"}


def question_plan(title: str, question: str) -> str:
    """A prototype's plan: its question, which is all of it."""
    return f"# {title}\n\n## Question\n\n{question.strip()}\n"


def prototype(request: CallRequest) -> dict[str, object]:
    """Add the prototype's task under `parent`, write its change in build with its question as the plan, and brief it.

    The task is added before the change is written, so a refused write leaves
    the task behind; drop it with `board call tasks.drop`.
    """
    asked = request.input
    mount, question = str(asked["mount"]).rstrip("/"), str(asked["question"])
    title = str(asked.get("title") or question)
    path = f"/changes/{asked.get('name') or name_of(title)}"
    board = Board(request.board.root, PLUGIN)
    exists(board, mount)
    refuse_existing(board, path)
    refuse_full(board, "prototype", path)
    task = add_task(board, title, {}, parent=str(asked["parent"]))
    _, lines = plan_lines(path, question_plan(title, question), [])
    change: Change = {
        "$type": CHANGE_TYPE,
        "title": title,
        "mount": mount,
        "phase": "build",
        "kind": "prototype",
        "task": task,
        "question": question,
        "state": "exploring",
        "plan": lines[0]["path"],
    }
    write(board, [*lines, {"path": path, "expect": "none", "record": change}])
    brief = brief_of(replace(request, input={"path": path}), path)
    return {"path": path, "task": task, "branch": branch_of(path), "brief": brief}


def verdict(request: CallRequest) -> dict[str, object]:
    """Record what the prototype found, and end it and its task: done when settled, dropped when abandoned."""
    asked = request.input
    path, state = str(asked["path"]), str(asked["state"])
    board = Board(request.board.root, PLUGIN)
    change, blob = read(board, path)
    if kind_of(change) != "prototype":
        raise PluginError(
            "changes::prototype::not_prototype",
            f"{path} is a {kind_of(change)} change, and only a prototype takes a verdict; "
            f"move it on with `board call changes.next --path {path}`",
            {"path": path, "kind": kind_of(change)},
        )
    if change["phase"] in FINAL:
        raise PluginError(
            "changes::phase::final",
            f"the prototype at {path} is {change['phase']}, so its verdict is already recorded; "
            "start another with `board call changes.prototype`",
            {"path": path, "phase": change["phase"]},
        )
    change |= {"state": state, "verdict": str(asked["verdict"]), "phase": ENDS[state]}
    artifact = asked.get("artifact")
    if isinstance(artifact, str):
        change["verdict_artifact"] = artifact
    write(board, [{"path": path, "expect": blob, "record": change}])
    if "task" in change:
        close_task(board, str(change["task"]), state)
    return {"path": path, "state": state, "phase": change["phase"]}
