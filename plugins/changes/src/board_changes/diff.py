"""The files a change edits, read with git from the mounted repository, and those no planned path matches.

A change's diff runs from where its head left the branch the mount follows
to its head, so it holds every commit of the change and none the branch
gained since. git runs without the user's or the system's config, so an
alias, an external diff driver or a rename setting cannot change which
files it lists.
"""

import json
from typing import Any

from board_plugin.board import Board, literal
from board_plugin.git import Failed, git


def edited(board: Board, mount: str, head: str) -> tuple[list[str], str | None]:
    """Each file the change at `head` adds, changes or deletes in `mount`'s repository, by logical path; or why not.

    The diff runs from the merge base of `head` and the branch the mount
    follows, which Board reports in the location it resolves `mount` to. A
    rename counts as deleting one path and adding another, so both must be
    planned.
    """
    [answer] = board.refs([mount])
    if answer is None:
        return [], f"nothing is at {mount}, so the change's diff cannot be read; check the mount with `board ls /repos`"
    location: dict[str, Any] = answer["location"]
    pointers: list[dict[str, Any]] = location.get("pointers") or []
    follow = pointers[-1].get("follow") if pointers else None
    if not isinstance(follow, str):
        return [], f"{mount} is not a mount, so it follows no branch to diff from; set the change's mount to a mount"
    repository, inside = str(location["repository"]), str(location.get("path") or "")
    names = git(
        repository, "diff", "--name-only", "-z", "--no-renames", "--merge-base", "--end-of-options", follow, head
    )
    if isinstance(names, Failed):
        if not names.started or names.timed_out:
            return [], names.why
        return [], f"{names.why}; fetch {head} and {follow} into the mounted repository"
    prefix = f"{inside}/" if inside else ""
    files = [name for name in names.split("\0") if name]
    return [f"{mount.rstrip('/')}/{name.removeprefix(prefix)}" for name in files if name.startswith(prefix)], None


def outside(board: Board, files: list[str], planned: list[str]) -> list[str]:
    """The files no planned path matches, compared with SQLite's GLOB as controls match their paths."""
    if not files:
        return []
    rows = board.rows(
        f"SELECT f.value FROM json_each({literal(json.dumps(files))}) AS f"  # noqa: S608 - values are quoted by literal()
        f" WHERE NOT EXISTS (SELECT 1 FROM json_each({literal(json.dumps(planned))}) AS p WHERE f.value GLOB p.value)"
        " ORDER BY f.key"
    )
    return [str(row[0]) for row in rows]
