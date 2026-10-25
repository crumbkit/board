"""Notes repos, their `notes-repo.json` records, and the choice of one for a call (README.md, Routing a note)."""

import json
import os
import re
import subprocess
from dataclasses import dataclass
from typing import Any

from board_plugin.board import Board, literal
from board_plugin.protocol import PluginError

RECORD = "notes-repo.json"
RECORD_TYPE = "notes/repo/v1"
# What `unpushed` and `unsynced` say for the board's own tree, which notes never syncs.
NOT_SYNCED = "notes::repo::not_synced"
# The sync refusals that leave the clone's HEAD where it was: a write there can still be made.
UNSYNCED = ("board::sync::conflict", "board::sync::edited")
# A GitHub remote as git names it, over ssh through a host alias such as github.com-work, or `owner/repo`:
# the owner and the repository's name.
GITHUB = re.compile(
    r"(?:https?://(?:[^@/]+@)?github\.com/|(?:ssh|git)://(?:[^@/]+@)?github\.com(?:-[\w.-]+)?/|[^@/:]+@github\.com(?:-[\w.-]+)?:)?"
    r"([A-Za-z0-9-]+)/([A-Za-z0-9._-]+?)(?:\.git)?/?"
)
# The user and password of a URL, or the user of an scp-style remote, which may hold a token.
USERINFO = re.compile(r"^([A-Za-z][A-Za-z0-9+.-]*://)?[^/]*@")
# Each notes repo's root, and its record's type, status, Board code and body when it has one.
REPOS = f"""
SELECT DISTINCT n.root, e.type, e.status, e.code, r.body FROM (
  SELECT DISTINCT source, substr(logical, 1, length(logical) - length(path)) AS root FROM entries
  WHERE path GLOB 'daily/*' AND substr(logical, length(logical) - length(path) + 1) = path
) AS n
LEFT JOIN entries AS e ON e.source = n.source AND e.path = {literal(RECORD)}
LEFT JOIN records AS r ON r.source = n.source AND r.path = {literal(RECORD)}
ORDER BY n.root
"""  # noqa: S608 - the only values interpolated are this module's constants, quoted by `literal`.


@dataclass(frozen=True)
class Note:
    """A note's text, and the blob that holds it, at the pin; both None when there is no note."""

    text: str | None
    blob: str | None


@dataclass(frozen=True)
class Repo:
    """The notes repo at `root`, `/` or a mount's path; `clone` is None for the board's own tree, never synced."""

    board: Board
    root: str
    clone: str | None

    def sync(self, *, push: bool) -> str | None:
        """Sync the clone with its remote: None, or the code of a refusal that left HEAD where it was."""
        try:
            self.board.document("-C", str(self.clone), "sync", *([] if push else ["--no-push"]))
        except PluginError as error:
            code = error.fields.get("code")
            if code in UNSYNCED:
                return str(code)
            raise
        return None

    def pin(self) -> str | None:
        """Move the mount's pin to the clone's HEAD and return that commit; None for the board's own tree."""
        if self.clone is None:
            return None
        return str(self.board.document("mount", "update", self.root)["data"]["new"])

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

    def logical(self, path: str) -> str:
        """The logical path of `path`, relative to the repo's root."""
        return self.root.rstrip("/") + "/" + path


@dataclass(frozen=True)
class Choice:
    """The repo a note goes to, and the repos it might belong to when it goes to the inbox; None otherwise."""

    repo: Repo
    candidates: list[str] | None


def choose(board: Board, name: str | None, remote: str | None, *, inbox: bool) -> Choice:
    """The notes repo at `name`; else the one the remote routes to; else the personal one's (README.md, Routing)."""
    rows = board.rows(REPOS)
    repos = [_root(str(row[0])) for row in rows]
    name = None if name is None else _root(name)
    if not repos or (name is not None and name not in repos):
        listed = ", ".join(repos) or "none"
        why = (
            f"no notes repo is at {name or 'any path'} (notes repos: {listed}); a notes repo is the board's root or a "
            "mount whose tree has a tracked file under daily/, so mount the repo, or name one listed with --repo"
        )
        raise PluginError("notes::repo::not_found", why, {"repos": repos})
    if name is not None:
        return Choice(_repo(board, name), None)
    records = {root: _record(root, *row[1:]) for root, row in zip(repos, rows, strict=True)}
    matched = [root for root, record in records.items() if covers(record, remote)]
    personal = [root for root, record in records.items() if record and record.get("personal") is True]
    if len(matched) == 1:
        return Choice(_repo(board, matched[0]), None)
    # No remote may mean a missing origin or a git that timed out, so a repo that covers something waits for one.
    if len(repos) == 1 and (personal or (remote is None and records[repos[0]] is None)):
        return Choice(_repo(board, repos[0]), None)
    if not inbox and remote is None and len(personal) == 1:
        return Choice(_repo(board, personal[0]), None)
    shown = "no git remote" if remote is None else shown_remote(remote)
    if not inbox:
        why = (
            f"{len(matched)} notes repos cover {shown} ({', '.join(matched) or 'none'}), so no daily note is "
            f"read; name one of {', '.join(repos)} with --repo"
        )
        raise PluginError("notes::repo::ambiguous", why, {"candidates": matched, "repos": repos})
    if len(personal) != 1:
        why = (
            f"{len(matched)} notes repos cover {shown}, so the note goes to the personal repo's inbox/, but "
            f"{len(personal)} of {', '.join(repos)} say personal: true in {RECORD}; nothing was written; mark "
            "exactly one, add the remote to one repo's covers, or name one with --repo"
        )
        raise PluginError("notes::repo::no_personal", why, {"repos": repos, "personal": personal})
    return Choice(_repo(board, personal[0]), matched)


def shown_remote(remote: str) -> str:
    """`remote` as it may be shown and stored: `owner/name` for GitHub, else the URL without its user and password."""
    matched = GITHUB.fullmatch(remote)
    return f"{matched[1]}/{matched[2]}" if matched else USERINFO.sub(r"\1", remote)


def caller_remote() -> str | None:
    """The origin of the git repository Board was started in, from BOARD_CALLER_DIR; None outside one."""
    directory = os.environ.get("BOARD_CALLER_DIR")
    if not directory:
        return None
    try:
        ran = subprocess.run(  # noqa: S603 - this reads git's config only, and changes nothing
            ["git", "-C", directory, "remote", "get-url", "origin"],  # noqa: S607 - git is found on PATH
            capture_output=True,
            text=True,
            timeout=10,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired):
        return None
    return ran.stdout.strip() or None if ran.returncode == 0 else None


def covers(record: dict[str, Any] | None, remote: str | None) -> bool:
    """Whether the record covers the GitHub repository `remote` names, as `owner/*` or `owner/name`, in any case."""
    matched = GITHUB.fullmatch(remote or "")
    if record is None or matched is None:
        return False
    owner, name = matched[1].lower(), matched[2].lower()
    patterns = [str(pattern).lower().partition("/") for pattern in record.get("covers", [])]
    return any(each == owner and rest in ("*", name) for each, _, rest in patterns)


def _record(root: str, kind: str | None, status: str | None, code: str | None, body: object) -> dict[str, Any] | None:
    """The repo's record; None when it has none. A record that is not a valid `notes/repo/v1` is refused."""
    if kind is None and status is None:
        return None
    if kind == RECORD_TYPE and status == "ok":
        return json.loads(body) if isinstance(body, str) else body  # type: ignore[no-any-return]
    path = root.rstrip("/") + "/" + RECORD.removesuffix(".json")
    clone = "the board" if root == "/" else f"the clone mounted at {root}"
    why = (
        f"{path} is not a valid {RECORD_TYPE} record (type {kind}, status {status}, code {code}), so no note "
        f"can be routed; run `board -C <{clone}> plugin enable notes` so it holds the schema, fix the record, then "
        f"`board mount update {root}`"
    )
    raise PluginError("notes::repo::invalid_record", why, {"path": path, "code": code, "status": status})


def _root(path: str) -> str:
    """`path` without a trailing `/`, except the board's root itself."""
    return path.rstrip("/") or "/"


def _repo(board: Board, root: str) -> Repo:
    """The notes repo at `root`, with the clone a mount there reaches."""
    if root == "/":
        return Repo(board, root, None)
    answer = board.refs([root])[0]
    location: dict[str, Any] = (answer or {}).get("location") or {}
    if "repository" not in location:
        why = f"board refs gave no clone for the mount at {root}; check the mount with `board check {root}`"
        raise PluginError("notes::board::refused", why, {"path": root})
    return Repo(board, root, str(location["repository"]))
