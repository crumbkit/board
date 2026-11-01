"""The repository `check` checks when it is given no path: the one Board was started in."""

import os
import subprocess
from pathlib import Path

from board_plugin.board import Board, literal
from board_plugin.repos import caller_remote, shown_remote

from board_controls.shapes import REPO_TYPE

# The mounts in the board's own tree, each with the clone Board reads it from, by the mount's path.
MOUNTS = (
    "SELECT e.logical, s.repository FROM mounts m"
    " JOIN sources s ON s.source = m.target"
    " JOIN entries e ON e.source = m.source AND e.path = m.path"
    " WHERE m.source = 'worktree' AND e.logical IS NOT NULL ORDER BY e.logical"
)


def repository(board: Board, root: str) -> tuple[str | None, str]:
    """The logical path of the repository `BOARD_CALLER_DIR` is in, and, when there is none, why.

    It is `/` when the directory is in the board's own repository or Board
    names no directory. Otherwise it is the first mount, by path, whose clone
    shares the directory's git directory, as Board finds a mount from a
    directory, so a linked worktree matches too; else the repo record whose
    `remote` is the stored form of the directory's origin. Repositories are
    compared by the git directory a linked worktree shares with its main one.
    """
    directory = os.environ.get("BOARD_CALLER_DIR")
    if not directory:
        return "/", ""
    common = _common(directory)
    if common is None:
        return None, f"{directory} is in no git repository, so there is no repository to check; pass --path"
    if _common(root) == common:
        return "/", ""
    for logical, clone in board.rows(MOUNTS):
        if _common(str(clone)) == common:
            return str(logical), ""
    origin = caller_remote()
    if origin is not None:
        remote = shown_remote(origin)
        rows = board.rows(
            "SELECT logical FROM records"  # noqa: S608 - values are quoted by literal()
            f" WHERE type = {literal(REPO_TYPE)} AND source = 'worktree' AND status = 'ok'"
            f" AND json_extract(body, '$.remote') = {literal(remote)} ORDER BY logical"
        )
        if rows:
            return str(rows[0][0]), ""
    named = "" if origin is None else f", origin {shown_remote(origin)},"
    return None, (
        f"no mount or repo record in this board holds the repository at {directory}{named} so nothing was"
        " checked; pass --path, or record the repository with `board put /repos/<name>`"
    )


def _common(directory: str) -> Path | None:
    """The git directory the repository holding `directory` shares with its linked worktrees, or None outside one."""
    try:
        ran = subprocess.run(  # noqa: S603 - this reads git's state only, and changes nothing
            ["git", "-C", directory, "rev-parse", "--path-format=absolute", "--git-common-dir"],  # noqa: S607
            capture_output=True,
            text=True,
            timeout=10,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired):
        return None
    if ran.returncode != 0 or not ran.stdout.strip():
        return None
    return Path(ran.stdout.strip()).resolve()
