"""A vault: a repository of notes in Reflect's layout, mounted at `/vaults/<name>` and a board itself.

Every read and write goes through Board. `board sync` in the vault's clone
brings in what the phone and Reflect's desktop app pushed, `board mount
update` moves the pin to the clone's HEAD, `board get` reads through the
mount, and `board put --batch` writes through it, which commits in the vault
and moves the pin.
"""

import json
from dataclasses import dataclass
from typing import Any

from board_plugin.board import Board
from board_plugin.protocol import PluginError

VAULTS = "/vaults/"
# The sync refusals that leave the clone's HEAD where it was: a write there can still be made.
UNSYNCED = ("board::sync::conflict", "board::sync::edited")


@dataclass(frozen=True)
class Note:
    """A note's text, and the blob that holds it, at the pin; both None when there is no note."""

    text: str | None
    blob: str | None


@dataclass(frozen=True)
class Vault:
    """The vault mounted at `mount`, whose clone is at `repository`."""

    board: Board
    mount: str
    repository: str

    @classmethod
    def find(cls, board: Board, name: str | None) -> "Vault":
        """The vault named `name`, or the only one mounted when `name` is None."""
        names = [
            str(row[0]).removeprefix(VAULTS)
            for row in board.rows(
                "SELECT logical FROM entries WHERE source = 'worktree' AND kind = 'pointer'"
                " AND substr(logical, 1, 8) = '/vaults/' ORDER BY logical"
            )
            if "/" not in str(row[0]).removeprefix(VAULTS)
        ]
        if name is None and len(names) > 1:
            raise PluginError(
                "notes::vault::ambiguous", f"several vaults are mounted, {', '.join(names)}; name one with --vault"
            )
        chosen = name or (names[0] if names else None)
        if chosen not in names:
            mounted = ", ".join(names) or "none"
            why = (
                f"no vault is mounted at {VAULTS}{chosen or '<name>'} (mounted: {mounted}); add a board/mount/v1 record"
            )
            raise PluginError("notes::vault::not_found", why, {"vaults": names})
        mount = VAULTS + str(chosen)
        answer = board.refs([mount])[0]
        location: dict[str, Any] = answer["location"] if answer else {}
        pointers: list[dict[str, Any]] = location.get("pointers", [])
        if location.get("path") != "" or len(pointers) != 1 or pointers[0]["repository"] == location["repository"]:
            why = f"{mount} does not mount another repository; point its record's `repo` at the vault's clone"
            raise PluginError("notes::vault::not_a_mount", why, {"vault": mount})
        return cls(board, mount, str(location["repository"]))

    def sync(self, *, push: bool) -> str | None:
        """Sync the clone with its remote: None, or the code of a refusal that left HEAD where it was."""
        try:
            self.board.document("-C", self.repository, "sync", *([] if push else ["--no-push"]))
        except PluginError as error:
            code = error.fields.get("code")
            if code in UNSYNCED:
                return str(code)
            raise
        return None

    def pin(self) -> str:
        """Move the mount's pin to the clone's HEAD and return that commit."""
        return str(self.board.document("mount", "update", self.mount)["data"]["new"])

    def read(self, path: str) -> Note:
        """The note at `path` in the vault, at the pin."""
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

    def write(self, path: str, text: str, expect: str | None) -> None:
        """Write `text` at `path` if it still holds `expect`. The put commits in the vault and moves the pin."""
        line = {"path": self.logical(path), "expect": expect or "none", "text": text}
        self.board.document("put", "--batch", stdin=(json.dumps(line) + "\n").encode())

    def logical(self, path: str) -> str:
        """The logical path of `path`, relative to the vault's root."""
        return f"{self.mount}/{path}"
