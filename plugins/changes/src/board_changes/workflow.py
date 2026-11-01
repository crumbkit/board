"""Workflows: the two `seed` writes, and the one `start` chooses for a repository."""

import json
from typing import Any, cast

from board_plugin.board import Board, literal
from board_plugin.protocol import CallRequest, PluginError

from board_changes.change import PLUGIN, READABLE, WORKFLOW_TYPE, write

PERSONAL = "/workflows/personal"
WORK = "/workflows/work"


def step(name: str, actor: str, done_by: str) -> dict[str, str]:
    """One workflow step."""
    return {"name": name, "actor": actor, "done_by": done_by}


PLAN = step("plan", "agent", "plan")
APPROVE = step("approve", "owner", "answer")
BUILD = step("build", "agent", "check")
REVIEW = step("review", "reviewer", "review")
LAND = step("land", "owner", "landed")
# The workflows `seed` writes when they are missing: a personal one, and one for work, which the owner approves.
SEEDS = {PERSONAL: [PLAN, BUILD, REVIEW, LAND], WORK: [PLAN, APPROVE, BUILD, REVIEW, LAND]}


def seed(request: CallRequest) -> dict[str, object]:
    """Write each seed workflow that is missing, in one write, and leave any already there as it is."""
    board = Board(request.board.root, PLUGIN)
    held = board.refs(list(SEEDS))
    missing = [path for path, answer in zip(SEEDS, held, strict=True) if answer is None]
    if missing:
        write(
            board,
            [
                {"path": path, "expect": "none", "record": {"$type": WORKFLOW_TYPE, "steps": SEEDS[path]}}
                for path in missing
            ],
        )
    return {"written": missing}


def steps_of(board: Board, path: str) -> list[dict[str, Any]] | None:
    """The steps of the readable workflow at `path`, or None when none is there."""
    rows = board.rows(
        f"SELECT body FROM records WHERE type = {literal(WORKFLOW_TYPE)} AND logical = {literal(path)}"  # noqa: S608 - values are quoted by literal()
        f" AND {READABLE}"
    )
    return cast("list[dict[str, Any]]", json.loads(rows[0][0])["steps"]) if rows else None


def target(board: Board, path: str) -> str | None:
    """Where `path` resolves, through any link, as a logical path; None when nothing is there."""
    [answer] = board.refs([path])
    return None if answer is None else str(answer["location"]["display"])


def choose(board: Board, given: str | None, name: str, owner: str | None) -> str:
    """The workflow of a change to the repository `name` whose remote's owner is `owner`.

    `given` wins. Otherwise the link `/workflows/repos/<name>` decides, else
    `/workflows/owners/<owner>`, each by the workflow it resolves to. With
    neither, `changes::workflow::unchosen`. A choice that holds no workflow
    is `changes::workflow::not_found`.
    """
    candidates = [given] if given else [f"/workflows/repos/{name}", *([f"/workflows/owners/{owner}"] if owner else [])]
    for candidate in candidates:
        resolved = target(board, candidate)
        if resolved is None and not given:
            continue
        if resolved is None or steps_of(board, resolved) is None:
            raise PluginError(
                "changes::workflow::not_found",
                f"{candidate} holds no {WORKFLOW_TYPE}; name a workflow such as {PERSONAL}, which"
                " `board --personal call changes.seed` writes",
                {"workflow": candidate},
            )
        return resolved
    raise PluginError(
        "changes::workflow::unchosen",
        f"no workflow is chosen for this repository; put a link at {' or '.join(candidates)}, a board/mount/v1"
        f' such as {{"$type": "board/mount/v1", "path": "{PERSONAL}"}}, or pass --workflow',
        {"candidates": candidates},
    )
