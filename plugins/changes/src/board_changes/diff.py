"""The files a change edits, read with git from the mounted repository, and those no planned path matches.

A change's diff runs from where its head left the branch the mount follows
to its head, so it holds every commit of the change and none the branch
gained since. git runs without the user's or the system's config, so an
alias, an external diff driver or a rename setting cannot change which
files it lists.
"""

import json
import os
import subprocess
from pathlib import Path
from typing import Any

from board_plugin.board import Board, literal
from board_plugin.git import ISOLATED

# How long each git command may take.
GIT_SECONDS = 60


def edited(board: Board, mount: str, head: str) -> tuple[list[str], str | None]:
    """Each file the change at `head` adds, changes or deletes in `mount`'s repository, by logical path; or why not.

    The diff runs from the merge base of `head` and the branch the mount
    follows. A rename counts as deleting one path and adding another, so
    both must be planned.
    """
    [answer] = board.refs([mount])
    if answer is None:
        return [], f"nothing is at {mount}, so the change's diff cannot be read; check the mount with `board ls /repos`"
    location = answer["location"]
    repository, inside = str(location["repository"]), str(location.get("path") or "")
    follow, why = followed(location)
    if follow is None:
        return [], why
    base, why = git(repository, "merge-base", "--end-of-options", follow, head)
    if base is None:
        return [], f"{why}; fetch {head} and {follow} into the mounted repository"
    names, why = git(repository, "diff", "--name-only", "-z", "--no-renames", "--end-of-options", base.strip(), head)
    if names is None:
        return [], f"{why}; fetch {head} into the mounted repository"
    prefix = f"{inside}/" if inside else ""
    files = [name for name in names.split("\0") if name]
    return [f"{mount.rstrip('/')}/{name.removeprefix(prefix)}" for name in files if name.startswith(prefix)], None


def followed(location: dict[str, Any]) -> tuple[str | None, str]:
    """The ref the mount `location` reached through follows, as its pointer record names it; or why there is none."""
    pointers: list[dict[str, Any]] = location.get("pointers") or []
    if not pointers:
        return None, "the change's repository is not a mount, so it follows no branch to diff from"
    pointer = pointers[-1]
    board, path, source = (str(pointer.get(key) or "") for key in ("repository", "path", "source"))
    if source == "worktree":
        try:
            text = (Path(board) / path).read_text()
        except OSError as error:
            return None, f"the mount {path} cannot be read: {error}"
    else:
        shown, why = git(board, "show", "--end-of-options", f"{source}:{path}")
        if shown is None:
            return None, why
        text = shown
    try:
        follow = json.loads(text).get("follow")
    except (ValueError, AttributeError):
        follow = None
    if not isinstance(follow, str) or not follow:
        return None, f"the mount {path} follows no branch, so the change's diff has no base; give it a `follow` ref"
    return follow, ""


def git(repository: str, *arguments: str) -> tuple[str | None, str]:
    """What `git -C <repository> <arguments>` prints, or `None` and why it failed."""
    command = ["git", "-C", repository, *arguments]
    try:
        ran = subprocess.run(  # noqa: S603 - the commits and refs come from records Board validated
            command,
            env={**os.environ, **ISOLATED},
            capture_output=True,
            check=False,
            timeout=GIT_SECONDS,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        return None, f"`git {' '.join(arguments)}` could not run in {repository}: {error}"
    if ran.returncode != 0:
        stderr = ran.stderr.decode(errors="replace").strip()
        return None, f"`git {' '.join(arguments)}` failed in {repository}: {stderr}"
    return ran.stdout.decode(errors="replace"), ""


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
