"""Writing a file of a notes repo through `board put`, and the refusals a write retries on.

The notes and tasks plugins write this way. plugins/notes/README.md says how a note's write is retried.
"""

from typing import cast

from board_plugin.protocol import PluginError
from board_plugin.repos import Repo

STALE = "board::put::stale"
# A put to the board's own tree refused because HEAD moved while it wrote: the file is written, and not committed.
HEAD_MOVED = "board::put::head_moved"
# Refusals of a write because the file or the clone moved after the read; the write is tried again on a new read.
MOVED = (STALE, HEAD_MOVED)
# The refusal of a write over an edit not committed in the clone, which no new read clears.
EDITED = "board::mount::stale"
ATTEMPTS = 3


def refused(error: PluginError) -> object:
    """The code Board refused a write with. A batch reports its one line's refusal in `refusals`."""
    refusals = error.fields.get("refusals")
    if isinstance(refusals, list) and len(cast("list[object]", refusals)) == 1:
        return cast("list[dict[str, object]]", refusals)[0].get("code")
    return error.fields.get("code")


def create(repo: Repo, stem: str, text: str, *, names: int, busy: str) -> tuple[str, str | None]:
    """Write `text` to a new file at `<stem>.md`, or at `<stem>-2.md` and on when one is there: its path and commit.

    It tries `names` names, then refuses with the code `busy`.
    """
    number, moved = 1, 0
    while number <= names:
        path = f"{stem}.md" if number == 1 else f"{stem}-{number}.md"
        repo.pin()
        try:
            return path, repo.write(path, text, None)
        except PluginError as error:
            code = refused(error)
            if code == STALE and moved:
                # The put whose HEAD moved wrote this file, so it is this file.
                return path, None
            if code == HEAD_MOVED:
                moved += 1
                if repo.clone is None or moved == ATTEMPTS:
                    # Board wrote the file before HEAD moved, so it is in the working tree, uncommitted.
                    return path, None
            elif code == STALE:
                number += 1
            else:
                raise
    why = f"{repo.logical(stem)}.md and the {names - 1} names after it are taken; nothing was written; call again"
    raise PluginError(busy, why, {"path": repo.logical(f"{stem}.md")})


def push(repo: Repo) -> str | None:
    """Sync the clone, pushing: None, or the code of the refusal.

    The code is `<plugin>::repo::not_synced` for the board's own tree, never synced. A failed push after a
    write is not retried, since the file is committed.
    """
    if repo.clone is None:
        return f"{repo.board.plugin}::repo::not_synced"
    try:
        return repo.sync(push=True)
    except PluginError as error:
        return str(error.fields.get("code", error.code))


def landed(repo: Repo, commit: str | None) -> tuple[str | None, str | None]:
    """Push a write, then move the mount's pin to the clone's HEAD: the commit holding the write, and `push`'s code.

    A put through a mount pins the commit it made, and the push may rebase that commit onto the remote's, so the
    pin moves after the push and the commit returned is the clone's HEAD. `commit` is None when the put left the
    write uncommitted.
    """
    unpushed = push(repo)
    return (None if commit is None else repo.pin() or commit), unpushed
