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
from typing import Any

from board_plugin.board import Board, literal
from board_plugin.protocol import PluginError

RECORD = "notes-repo.json"
RECORD_TYPE = "notes/repo/v1"
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


def listed(board: Board) -> list[tuple[str, tuple[Any, ...]]]:
    """Every notes repo the board holds, by root: its root, and its record's type, status, code and body."""
    return [(root(str(row[0])), tuple(row[1:])) for row in board.rows(REPOS)]


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


def clone(board: Board, root: str) -> str | None:
    """The clone the mount at `root` reaches; None for the board's own tree."""
    if root == "/":
        return None
    answer = board.refs([root])[0]
    location: dict[str, Any] = (answer or {}).get("location") or {}
    if "repository" not in location:
        why = f"board refs gave no clone for the mount at {root}; check the mount with `board check {root}`"
        raise PluginError(f"{board.plugin}::board::refused", why, {"path": root})
    return str(location["repository"])


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


def root(path: str) -> str:
    """`path` without a trailing `/`, except the board's root itself."""
    return path.rstrip("/") or "/"
