"""The files a reviewed range edits, read with git from the mounted repository, and those no planned path matches.

git runs without the user's or the system's config, so an alias, an external
diff driver or a rename setting cannot change which files it lists.
"""

import json
import os
import subprocess

from board_plugin.board import Board, literal

# The variables that keep the user's and the system's git config out of every git command.
QUIET = {"GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": os.devnull, "GIT_TERMINAL_PROMPT": "0"}
# How long git may take to list a range's files.
GIT_SECONDS = 60


def edited(board: Board, mount: str, base: str, head: str) -> tuple[list[str], str | None]:
    """Each file `base...head` adds, changes or deletes in `mount`'s repository, by logical path; or why not.

    A rename counts as deleting one path and adding another, so both must be planned.
    """
    [answer] = board.refs([mount])
    if answer is None:
        return [], f"nothing is at {mount}, so the reviewed diff cannot be read; check the mount with `board ls /repos`"
    location = answer["location"]
    repository, inside = str(location["repository"]), str(location.get("path") or "")
    try:
        arguments = [
            "-C",
            repository,
            "diff",
            "--name-only",
            "-z",
            "--no-renames",
            "--end-of-options",
            f"{base}...{head}",
        ]
        ran = subprocess.run(  # noqa: S603 - the commits come from a review report Board validated
            ["git", *arguments],  # noqa: S607 - git is found on PATH
            env={**os.environ, **QUIET},
            capture_output=True,
            check=False,
            timeout=GIT_SECONDS,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        return [], f"git could not list the files {base}...{head} edits in {repository}: {error}"
    if ran.returncode != 0:
        stderr = ran.stderr.decode(errors="replace").strip()
        return [], (
            f"git could not list the files {base}...{head} edits in {repository}: {stderr}; fetch both commits "
            "into the mounted repository"
        )
    prefix = f"{inside}/" if inside else ""
    files = [name.decode(errors="replace") for name in ran.stdout.split(b"\0") if name]
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
