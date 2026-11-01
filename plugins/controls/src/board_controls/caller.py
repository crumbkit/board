"""The repository `check` checks when it is given no path: the one Board was started in."""

import os

from board_plugin.board import Board
from board_plugin.repos import caller_remote, caller_repo, common_dir, shown_remote


def repository(board: Board, root: str) -> tuple[str | None, str]:
    """The logical path of the repository `BOARD_CALLER_DIR` is in, and, when there is none, why.

    It is `/` when the directory is in the board's own repository or Board
    names no directory. Otherwise it is the repository
    `board_plugin.repos.caller_repo` finds: a mount whose clone shares the
    directory's git directory, else the repo record whose `remote` is the
    stored form of the directory's origin.
    """
    directory = os.environ.get("BOARD_CALLER_DIR")
    if not directory:
        return "/", ""
    common = common_dir(directory)
    if common is None:
        return None, f"{directory} is in no git repository, so there is no repository to check; pass --path"
    if common_dir(root) == common:
        return "/", ""
    found = caller_repo(board)
    if found is not None:
        return found, ""
    origin = caller_remote()
    named = "" if origin is None else f", origin {shown_remote(origin)},"
    return None, (
        f"no mount or repo record in this board holds the repository at {directory}{named} so nothing was"
        " checked; pass --path, or record the repository with `board put /repos/<name>`"
    )
