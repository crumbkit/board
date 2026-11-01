"""The capabilities that write one record of a change: `claim`, `files`, `check`, `landed` and `drop`."""

from typing import Any, cast

from board_plugin.board import Board
from board_plugin.protocol import CallRequest, PluginError

from board_changes import clone
from board_changes.change import (
    CHECK_TYPE,
    CLAIM_TYPE,
    DROPPED_TYPE,
    FILES_TYPE,
    LANDED_TYPE,
    PLUGIN,
    RUN_TYPE,
    Change,
    live,
    referenced,
    stamp,
    write,
)
from board_changes.steps import repo_name


def opened(request: CallRequest) -> tuple[Board, str, dict[str, Any], Change]:
    """The board, the change's path, the input and the change, once the change is known not to have ended."""
    board = Board(request.board.root, PLUGIN)
    path = str(request.input["path"]).rstrip("/")
    change, _ = live(board, path)
    return board, path, request.input, change


def put(board: Board, path: str, record: dict[str, Any], *, new: bool = True) -> None:
    """Write `record` at `path`; with `new`, only where nothing is yet."""
    line: dict[str, Any] = {"path": path, "record": record}
    if new:
        line["expect"] = "none"
    write(board, [line])


def claim_record(session: str) -> dict[str, Any]:
    """A claim for `session` naming the root of the caller's clone."""
    return {"$type": CLAIM_TYPE, "session": session, "worktree": clone.root()}


def claim(request: CallRequest) -> dict[str, object]:
    """Write a claim that `session` holds the change, in the clone Board was started in."""
    board, path, given, _ = opened(request)
    record = claim_record(str(given["session"]))
    written = f"{path}/claims/{stamp()}"
    put(board, written, record)
    return {"path": written, "worktree": record["worktree"]}


def files(request: CallRequest) -> dict[str, object]:
    """Write the paths the branch touches at `head`, replacing an earlier list for that head."""
    board, path, given, _ = opened(request)
    written = f"{path}/files/{given['head']}"
    put(
        board,
        written,
        {"$type": FILES_TYPE, "files": list(dict.fromkeys(cast("list[str]", given["files"])))},
        new=False,
    )
    return {"path": written}


def check(request: CallRequest) -> dict[str, object]:
    """Write a check of the run, which must name the head under the repo record's name in its `versions`."""
    board, path, given, change = opened(request)
    run = str(given["run"]).rstrip("/")
    found, _ = referenced(board, [run])
    kind, body = found.get(run, ("", {}))
    if kind != RUN_TYPE:
        raise PluginError(
            "changes::run::not_found",
            f"no readable {RUN_TYPE} is at {run}; record the suite's run first, and name it by its logical path",
            {"run": run},
        )
    versions: dict[str, Any] = body.get("versions") or {}
    name = repo_name(change)
    head = versions.get(name)
    if not isinstance(head, str):
        raise PluginError(
            "changes::run::no_commit",
            f"the run at {run} names no commit under {name!r} in its versions, so it names no head; record the"
            f" commit it tested as versions.{name}",
            {"run": run, "name": name},
        )
    written = f"{path}/checks/{stamp()}"
    put(board, written, {"$type": CHECK_TYPE, "run": run})
    return {"path": written, "head": head}


def landed(request: CallRequest) -> dict[str, object]:
    """End the change as merged at `commit`."""
    board, path, given, _ = opened(request)
    written = f"{path}/landed"
    put(board, written, {"$type": LANDED_TYPE, "commit": str(given["commit"])})
    return {"path": written}


def drop(request: CallRequest) -> dict[str, object]:
    """End the change as stopped, saying why."""
    board, path, given, _ = opened(request)
    written = f"{path}/dropped"
    put(board, written, {"$type": DROPPED_TYPE, "why": str(given["why"])})
    return {"path": written}
