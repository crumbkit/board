"""Reading and writing notes in a notes repo, and the choice of one for a call (README.md, Routing a note).

Discovery and remote matching are shared with the tasks plugin, in `board_plugin.repos`.
"""

from dataclasses import dataclass

from board_plugin import repos
from board_plugin.board import Board
from board_plugin.protocol import PluginError
from board_plugin.repos import RECORD, shown_remote

# What `unpushed` and `unsynced` say for the board's own tree, which notes never syncs.
NOT_SYNCED = "notes::repo::not_synced"


@dataclass(frozen=True)
class Repo(repos.Repo):
    """A notes repo whose notes a call reads and writes."""

    def read(self, path: str) -> repos.File:
        """The note at `path` in the repo, at the pin, refused when it is not UTF-8 text."""
        note = super().read(path)
        if note.text is None and note.blob is not None:
            logical = self.logical(path)
            why = f"{logical} is not UTF-8 text, so no section can be appended to it; nothing was written"
            raise PluginError("notes::note::not_text", why, {"path": logical})
        return note


@dataclass(frozen=True)
class Choice:
    """The repo a note goes to, and the repos it might belong to when it goes to the inbox; None otherwise."""

    repo: Repo
    candidates: list[str] | None


def choose(board: Board, name: str | None, remote: str | None, *, inbox: bool) -> Choice:
    """The notes repo at `name`; else the one the remote routes to; else the personal one's (README.md, Routing)."""
    found = repos.listed(board)
    roots = [at for at, *_ in found]
    at_root = {at: Repo(board, at, clone) for at, _, clone, _ in found}
    name = None if name is None else repos.root(name)
    if not roots or (name is not None and name not in roots):
        listed = ", ".join(roots) or "none"
        why = (
            f"no notes repo is at {name or 'any path'} (notes repos: {listed}); a notes repo is the board's root or a "
            "mount whose tree has a tracked file under daily/, so mount the repo, or name one listed with --repo"
        )
        raise PluginError("notes::repo::not_found", why, {"repos": roots})
    if name is not None:
        return Choice(at_root[name], None)
    matched, personal = repos.covering(board.plugin, found, remote)
    if len(matched) == 1:
        return Choice(at_root[matched[0]], None)
    # No remote may mean a missing origin or a git that timed out, so a repo that covers something waits for one.
    if len(roots) == 1 and (personal or (remote is None and repos.record(board.plugin, roots[0], found[0][3]) is None)):
        return Choice(at_root[roots[0]], None)
    if not inbox and remote is None and len(personal) == 1:
        return Choice(at_root[personal[0]], None)
    shown = "no git remote" if remote is None else shown_remote(remote)
    if not inbox:
        why = (
            f"{len(matched)} notes repos cover {shown} ({', '.join(matched) or 'none'}), so no daily note is "
            f"read; name one of {', '.join(roots)} with --repo"
        )
        raise PluginError("notes::repo::ambiguous", why, {"candidates": matched, "repos": roots})
    if len(personal) != 1:
        why = (
            f"{len(matched)} notes repos cover {shown}, so the note goes to the personal repo's inbox/, but "
            f"{len(personal)} of {', '.join(roots)} say personal: true in {RECORD}; nothing was written; mark "
            "exactly one, add the remote to one repo's covers, or name one with --repo"
        )
        raise PluginError("notes::repo::no_personal", why, {"repos": roots, "personal": personal})
    return Choice(at_root[personal[0]], matched)
