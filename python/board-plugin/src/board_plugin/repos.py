"""Notes repos, their `notes-repo.json` records, and the git remotes the records cover.

A notes repo is the board's root or a mount whose tree has a tracked file
under `daily/`, as in Reflect's layout. Its `notes/repo/v1` record, which the
notes plugin ships, names the GitHub repositories whose work it is about.
plugins/notes/README.md, under Routing a note, says how notes uses them.
"""

import json
import os
import re
import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from board_plugin.board import Board, literal
from board_plugin.protocol import PluginError

RECORD = "notes-repo.json"
RECORD_TYPE = "notes/repo/v1"
REPO_TYPE = "board/repo/v1"
# The mounts in the board's own tree, each with the clone Board reads it from, by the mount's path.
MOUNTS = (
    "SELECT e.logical, s.repository FROM mounts m"
    " JOIN sources s ON s.source = m.target"
    " JOIN entries e ON e.source = m.source AND e.path = m.path"
    " WHERE m.source = 'worktree' AND e.logical IS NOT NULL ORDER BY e.logical"
)
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
# Each notes repo's root, the commit its mount pins and the clone Board read it from (`worktree` and the
# board's own root for `/`), and its record's type, status, Board code and body when it has one.
COLUMNS = ("n.root", "n.source", "s.repository", "e.type", "e.status", "e.code", "r.body")
REPOS = f"""
SELECT DISTINCT {", ".join(COLUMNS)} FROM (
  SELECT DISTINCT source, substr(logical, 1, length(logical) - length(path)) AS root FROM entries
  WHERE path GLOB 'daily/*' AND substr(logical, length(logical) - length(path) + 1) = path
) AS n
LEFT JOIN sources AS s ON s.source = n.source
LEFT JOIN entries AS e ON e.source = n.source AND e.path = {literal(RECORD)}
LEFT JOIN records AS r ON r.source = n.source AND r.path = {literal(RECORD)}
ORDER BY n.root
"""  # noqa: S608 - the only values interpolated are this module's constants, quoted by `literal`.


# A notes repo: its root, the commit its mount pins, the clone that holds it (both None for the board's own
# tree), and its record's type, status, code and body (all None without one).
Listed = tuple[str, str | None, str | None, tuple[Any, ...]]


def listed(board: Board) -> list[Listed]:
    """Every notes repo the board holds, by root."""
    return parsed(board.plugin, board.rows(REPOS))


def parsed(plugin: str, rows: list[list[Any]]) -> list[Listed]:
    """The notes repos in the rows of `REPOS`, by root, for a caller that ran `REPOS` within a query of its own.

    Board's `sources` table names the clone a commit was first read from. Board indexes a commit once, so a
    second mount of a commit another mount already pins has no entries and is not listed at all.
    """
    found: list[Listed] = []
    for path, source, repository, *row in rows:
        at = root(str(path))
        if source != "worktree" and repository is None:
            why = f"Board names no clone for the mount at {at}; check the mount with `board check {at}`"
            raise PluginError(f"{plugin}::board::refused", why, {"path": at})
        mounted = source != "worktree"
        found.append((at, str(source) if mounted else None, str(repository) if mounted else None, tuple(row)))
    return sorted(found)


def record(plugin: str, at: str, row: tuple[Any, ...]) -> dict[str, Any] | None:
    """The record of the repo at `at`, from its `listed` row; None without one, and refused unless valid."""
    kind, status, code, body = row
    if kind is None and status is None:
        return None
    if kind == RECORD_TYPE and status == "ok":
        return json.loads(body) if isinstance(body, str) else body  # type: ignore[no-any-return]
    path = at.rstrip("/") + "/" + RECORD.removesuffix(".json")
    clone = "the board" if at == "/" else f"the clone mounted at {at}"
    why = (
        f"{path} is not a valid {RECORD_TYPE} record (type {kind}, status {status}, code {code}), so no call "
        f"can route by remote; run `board -C <{clone}> plugin enable notes` so it holds the schema, fix the "
        f"record, then `board mount update {at}`"
    )
    raise PluginError(f"{plugin}::repo::invalid_record", why, {"path": path, "code": code, "status": status})


@dataclass(frozen=True)
class File:
    """A file at the pin: its text, its blob, the record Board reads from it, and its location.

    The text is None when the file is not UTF-8. The location is as `board get` gives it, with the clone that
    holds the file, `repository`, and its `source`.

    All four are None when there is no file.
    """

    text: str | None
    blob: str | None
    record: dict[str, Any] | None = None
    location: dict[str, Any] | None = None


@dataclass(frozen=True)
class Repo:
    """The notes repo at `root`, `/` or a mount's path; `clone` is None for the board's own tree, never synced."""

    board: Board
    root: str
    clone: str | None

    def logical(self, path: str) -> str:
        """The logical path of `path`, relative to the repo's root."""
        return self.root.rstrip("/") + "/" + path

    def read(self, path: str) -> File:
        """The file at `path` in the repo, at the pin."""
        logical = self.logical(path)
        report = self.board.document("get", "--", logical, partial="board/get-report/v2")
        failures: list[dict[str, Any]] = report["failures"]
        found: list[dict[str, Any]] = report["data"]
        refused = f"{self.board.plugin}::board::refused"
        if failures:
            if failures[0].get("code") == "board::path::not_found":
                return File(None, None)
            failure = failures[0]
            raise PluginError(refused, str(failure.get("message")), {"code": failure.get("code")})
        if len(found) != 1:
            raise PluginError(refused, f"board get printed {len(found)} answers for {logical}")
        text, blob = found[0].get("text"), str(found[0]["blob"])
        return File(None if text is None else str(text), blob, found[0].get("record"), found[0].get("location"))

    def write(self, path: str, text: str, expect: str | None) -> str | None:
        """Write `text` at `path` if it still holds `expect`: the commit that holds it, None when uncommitted."""
        line = {"path": self.logical(path), "expect": expect or "none", "text": text}
        report = self.board.document("put", "--batch", stdin=(json.dumps(line) + "\n").encode())
        commit = report["data"]["commit"]
        return None if commit is None else str(commit)

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


def shown_remote(remote: str) -> str:
    """`remote` as it may be shown and stored: `owner/name` for GitHub, else the URL without its user and password.

    The form is reapplied until it holds still, so the stored form of a stored form is itself. Each change
    shortens the text, so this ends. board-model's `shown_remote` does the same, reading the same table.
    """
    shown = remote
    while True:
        matched = GITHUB.fullmatch(shown)
        following = f"{matched[1]}/{matched[2]}" if matched else USERINFO.sub(r"\1", shown)
        if following == shown:
            return shown
        shown = following


def caller_remote() -> str | None:
    """The origin of the git repository Board was started in, from BOARD_CALLER_DIR; None outside one."""
    return caller_origin()[0]


def caller_origin() -> tuple[str | None, str | None]:
    """The origin of the git repository Board was started in, and why there is none inside a repository.

    Both are None outside a git repository. Inside one whose origin git did not give, the reason names why:
    no origin is set, git failed or ran past its 10 s deadline.
    """
    directory = os.environ.get("BOARD_CALLER_DIR")
    if not directory:
        return None, None
    try:
        ran = subprocess.run(  # noqa: S603 - this reads git's config only, and changes nothing
            ["git", "-C", directory, "remote", "get-url", "origin"],  # noqa: S607 - git is found on PATH
            capture_output=True,
            text=True,
            timeout=10,
            check=False,
            env={**os.environ, "LC_ALL": "C"},  # git's messages in English, which the check below reads
        )
    except subprocess.TimeoutExpired:
        return None, f"git ran past 10 s reading the origin of {directory}"
    except OSError as error:
        return None, f"git cannot be run: {error}"
    if ran.returncode == 0 and ran.stdout.strip():
        return ran.stdout.strip(), None
    if "not a git repository" in ran.stderr:
        return None, None
    failed = ran.stderr.strip().splitlines()[-1:] or [f"git exited {ran.returncode}"]
    return None, f"git gave no origin for {directory}: {failed[0]}"


def common_dir(directory: str) -> Path | None:
    """The git directory the repository holding `directory` shares with its linked worktrees, or None outside one."""
    found = git(directory, "rev-parse", "--path-format=absolute", "--git-common-dir")
    return None if found is None else Path(found).resolve()


def caller_root() -> str | None:
    """The root of the clone or linked worktree BOARD_CALLER_DIR is in, or None outside one."""
    directory = os.environ.get("BOARD_CALLER_DIR")
    return None if not directory else git(directory, "rev-parse", "--show-toplevel")


def caller_repo(board: Board) -> str | None:
    """The logical path of the repository BOARD_CALLER_DIR is in, as a mount or repo record in the board's own tree.

    It is the first mount, by path, whose clone shares the directory's git
    directory, as Board finds a mount from a directory, so a linked worktree
    matches too; else the repo record whose `remote` is the stored form of
    the directory's origin. Board gives plugins no pointer's body, so a repo
    record with a checkout is found by its clone only. None when neither
    holds the repository, or outside a git repository.
    """
    directory = os.environ.get("BOARD_CALLER_DIR")
    common = None if not directory else common_dir(directory)
    if common is None:
        return None
    for logical, clone in board.rows(MOUNTS):
        if common_dir(str(clone)) == common:
            return str(logical)
    origin = caller_remote()
    if origin is None:
        return None
    rows = board.rows(
        "SELECT logical FROM records"  # noqa: S608 - values are quoted by literal()
        f" WHERE type = {literal(REPO_TYPE)} AND source = 'worktree' AND status = 'ok'"
        f" AND json_extract(body, '$.remote') = {literal(shown_remote(origin))} ORDER BY logical"
    )
    return str(rows[0][0]) if rows else None


def git(directory: str, *arguments: str) -> str | None:
    """What `git -C <directory> <arguments>` printed, trimmed; None when it failed, printed nothing or ran 10 s."""
    try:
        ran = subprocess.run(  # noqa: S603 - this reads git's state only, and changes nothing
            ["git", "-C", directory, *arguments],  # noqa: S607 - git is found on PATH
            capture_output=True,
            text=True,
            timeout=10,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired):
        return None
    return ran.stdout.strip() if ran.returncode == 0 and ran.stdout.strip() else None


def covering(plugin: str, found: list[Listed], remote: str | None) -> tuple[list[str], list[str]]:
    """The roots of the listed repos whose record covers `remote`, and of those whose record says `personal: true`."""
    records = {at: record(plugin, at, row) for at, _, _, row in found}
    covered = [at for at, each in records.items() if covers(each, remote)]
    return covered, [at for at, each in records.items() if (each or {}).get("personal") is True]


def covers(record: dict[str, Any] | None, remote: str | None) -> bool:
    """Whether the record covers the GitHub repository `remote` names, as `owner/*` or `owner/name`, in any case."""
    matched = GITHUB.fullmatch(remote or "")
    if record is None or matched is None:
        return False
    owner, name = matched[1].lower(), matched[2].lower()
    patterns = [str(pattern).lower().partition("/") for pattern in record.get("covers", [])]
    return any(each == owner and rest in ("*", name) for each, _, rest in patterns)


def scope(plugin: str, found: list[Listed], name: str | None, remote: str | None, unread: str | None) -> list[str]:
    """The repos to read, the board's own tree, `/`, among them.

    They are the one named; else every repo outside a git repository; else `/`, the personal repo and the
    repos whose record covers the remote. Inside a git repository whose origin is `unread`, no repo is
    covered, so they are `/` and the personal repo.
    """
    roots = ["/", *[at for at, *_ in found if at != "/"]]
    if name is not None:
        if root(name) not in roots:
            why = (
                f"no repo is at {name} (repos: {', '.join(roots)}); name the board's own tree, /, or one of the "
                "other repos listed"
            )
            raise PluginError(f"{plugin}::repo::not_found", why, {"repos": roots})
        return [root(name)]
    if remote is None and unread is None:
        return roots
    records = {at: record(plugin, at, row) for at, _, _, row in found}
    return [
        at for at in roots if at == "/" or (records[at] or {}).get("personal") is True or covers(records[at], remote)
    ]


def root(path: str) -> str:
    """`path` without a trailing `/`, except the board's root itself."""
    return path.rstrip("/") or "/"
