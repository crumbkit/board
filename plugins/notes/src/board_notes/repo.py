"""Reading and writing notes in a notes repo, and the choice of one for a call (README.md, Routing a note).

Discovery and remote matching are shared with the tasks plugin, in `board_plugin.repos`.
"""

import json
from dataclasses import dataclass
from typing import Any

from board_plugin import repos
from board_plugin.board import Board
from board_plugin.protocol import PluginError
from board_plugin.repos import RECORD, shown_remote

# What `unpushed` and `unsynced` say for the board's own tree, which notes never syncs.
NOT_SYNCED = "notes::repo::not_synced"


@dataclass(frozen=True)
class Note:
    """A note's text, and the blob that holds it, at the pin; both None when there is no note."""

    text: str | None
    blob: str | None


@dataclass(frozen=True)
class Repo(repos.Repo):
    """A notes repo whose notes a call reads and writes."""

    def logical(self, path: str) -> str:
        """The logical path of `path`, relative to the repo's root."""
        return self.root.rstrip("/") + "/" + path

    def read(self, path: str) -> Note:
        """The note at `path` in the repo, at the pin."""
        logical = self.logical(path)
        report = self.board.document("get", "--", logical, partial="board/get-report/v2")
        failures: list[dict[str, Any]] = report["failures"]
        found: list[dict[str, Any]] = report["data"]
        if failures:
            if failures[0].get("code") == "board::path::not_found":
                return Note(None, None)
            failure = failures[0]
            raise PluginError("notes::board::refused", str(failure.get("message")), {"code": failure.get("code")})
        if len(found) != 1:
            raise PluginError("notes::board::refused", f"board get printed {len(found)} answers for {logical}")
        if "text" not in found[0]:
            why = f"{logical} is not UTF-8 text, so no section can be appended to it; nothing was written"
            raise PluginError("notes::note::not_text", why, {"path": logical})
        return Note(str(found[0]["text"]), str(found[0]["blob"]))

    def write(self, path: str, text: str, expect: str | None) -> str | None:
        """Write `text` at `path` if it still holds `expect`: the commit that holds it, None when uncommitted."""
        line = {"path": self.logical(path), "expect": expect or "none", "text": text}
        report = self.board.document("put", "--batch", stdin=(json.dumps(line) + "\n").encode())
        commit = report["data"]["commit"]
        return None if commit is None else str(commit)


@dataclass(frozen=True)
class Choice:
    """The repo a note goes to, and the repos it might belong to when it goes to the inbox; None otherwise."""

    repo: Repo
    candidates: list[str] | None


def choose(board: Board, name: str | None, remote: str | None, *, inbox: bool) -> Choice:
    """The notes repo at `name`; else the one the remote routes to; else the personal one's (README.md, Routing)."""
    found = repos.listed(board)
    roots = [at for at, _ in found]
    name = None if name is None else repos.root(name)
    if not roots or (name is not None and name not in roots):
        listed = ", ".join(roots) or "none"
        why = (
            f"no notes repo is at {name or 'any path'} (notes repos: {listed}); a notes repo is the board's root or a "
            "mount whose tree has a tracked file under daily/, so mount the repo, or name one listed with --repo"
        )
        raise PluginError("notes::repo::not_found", why, {"repos": roots})
    if name is not None:
        return Choice(_repo(board, name), None)
    records = {at: repos.record(board.plugin, at, row) for at, row in found}
    matched = [root for root, record in records.items() if repos.covers(record, remote)]
    personal = [root for root, record in records.items() if record and record.get("personal") is True]
    if len(matched) == 1:
        return Choice(_repo(board, matched[0]), None)
    # No remote may mean a missing origin or a git that timed out, so a repo that covers something waits for one.
    if len(roots) == 1 and (personal or (remote is None and records[roots[0]] is None)):
        return Choice(_repo(board, roots[0]), None)
    if not inbox and remote is None and len(personal) == 1:
        return Choice(_repo(board, personal[0]), None)
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
    return Choice(_repo(board, personal[0]), matched)


def _repo(board: Board, root: str) -> Repo:
    """The notes repo at `root`, with the clone a mount there reaches."""
    return Repo(board, root, repos.clone(board, root))
