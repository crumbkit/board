"""Fixture boards that run the real `board` binary, which has decisions, evals and plans built in."""

import json
import os
import shutil
import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import pytest

PLUGIN = Path(__file__).resolve().parents[1]
REPOSITORY = PLUGIN.parents[1]
# The schema the plugin ships, kept in its crate.
SCHEMAS = REPOSITORY / "crates" / "board-plugin-decisions" / "types" / "decisions"


def board_binary() -> Path:
    """The `board` under test: `$BOARD_BIN`, or the workspace's debug build."""
    return Path(os.environ.get("BOARD_BIN", REPOSITORY / "target" / "debug" / "board")).resolve()


def flags(given: dict[str, Any]) -> list[str]:
    """`given` as the flags of `board call`; a value no flag can give, such as a list, is refused."""
    parts: list[str] = []
    for name, value in given.items():
        if not isinstance(value, str | int):
            message = f"{name} is {value!r}, which no flag can give"
            raise TypeError(message)
        parts += [f"--{name}", str(value)]
    return parts


@dataclass(frozen=True)
class Board:
    """A git repository holding a board that commits its puts, with its own data and cache directories."""

    root: Path
    data: Path
    cache: Path

    def write(self, path: str, record: dict[str, Any]) -> None:
        """Write a fixture record at `path`, relative to the board's root, and commit it."""
        target = self.root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(json.dumps(record, indent=2) + "\n")
        self.git("add", path)
        self.git("commit", "-qm", f"Write {path}")

    def git(self, *arguments: str) -> str:
        """Run git in the board's root and return what it printed."""
        identity = ["-c", "user.name=t", "-c", "user.email=t@t"]
        ran = subprocess.run(["git", *identity, *arguments], cwd=self.root, capture_output=True, text=True, check=True)
        return ran.stdout

    def environment(self) -> dict[str, str]:
        """The variables a user's shell would pass board, with the board under test first on PATH."""
        uv = shutil.which("uv")
        assert uv is not None, "uv is on PATH"
        path = os.pathsep.join([str(board_binary().parent), str(Path(uv).parent), "/usr/bin", "/bin"])
        return {
            "PATH": path,
            "HOME": os.environ.get("HOME", str(self.root)),
            "XDG_DATA_HOME": str(self.data),
            "XDG_CACHE_HOME": str(self.cache),
            "XDG_CONFIG_HOME": str(self.cache.parent / "config"),
        }

    def run(self, *arguments: str, stdin: str = "") -> subprocess.CompletedProcess[str]:
        """Run `board` in the board's root."""
        return subprocess.run(
            [str(board_binary()), *arguments],
            cwd=self.root,
            env=self.environment(),
            input=stdin,
            capture_output=True,
            text=True,
            check=False,
        )

    def json(self, *arguments: str, stdin: str = "") -> dict[str, Any]:
        """Run `board --json` and read its one document."""
        ran = self.run("--json", *arguments, stdin=stdin)
        document: dict[str, Any] = json.loads(ran.stdout)
        return document

    def call(self, target: str, given: dict[str, Any]) -> dict[str, Any]:
        """The output of `board call <target>`."""
        report = self.json("call", target, *flags(given))
        assert report["schema"] == "board/call-report/v2", report
        output: dict[str, Any] = report["data"]["output"]
        return output

    def checked(self) -> dict[str, str | None]:
        """Each record's code from `board check --all`, by its path; `None` when it is valid."""
        report = self.json("check", "--all")
        return {
            entry["logical"]: entry.get("code") for entry in report["data"] if not entry["logical"].startswith("/.")
        }

    def record(self, path: str) -> dict[str, Any]:
        """The record stored at `path`, relative to the board's root."""
        loaded: dict[str, Any] = json.loads((self.root / path).read_text())
        return loaded


@pytest.fixture
def board(tmp_path: Path) -> Board:
    """A committing board with decisions, evals and plans enabled through `board plugin enable`."""
    fixture = Board(root=tmp_path / "board", data=tmp_path / "data", cache=tmp_path / "cache")
    fixture.root.mkdir()
    fixture.git("init", "-q")
    # Board commits its puts, so the repository needs an identity to commit as.
    fixture.git("config", "user.name", "t")
    fixture.git("config", "user.email", "t@t")
    fixture.write(".board/board.json", {"$type": "board/config/v1"})
    for name in ("decisions", "evals", "plans"):
        enabled = fixture.run("plugin", "enable", name)
        assert enabled.returncode == 0, enabled.stderr
    return fixture


def decision(**fields: Any) -> dict[str, Any]:  # noqa: ANN401 - the record's other fields, as JSON
    """A `decisions/decision/v1` record."""
    return {
        "$type": "decisions/decision/v1",
        "decision": "Use gix for ref resolution.",
        "why": "It resolved 1,000 refs in 41 ms against 63 ms for git2.",
        "decided": "2026-10-20T09:00:00Z",
        "session": "local_335a90f9",
        **fields,
    }


def question(status: str = "open", belongs_to: list[str] | None = None) -> dict[str, Any]:
    """A `plans/question/v1` record, whose `for` is `belongs_to` when given."""
    record: dict[str, Any] = {
        "$type": "plans/question/v1",
        "question": "Do you accept these decisions?",
        "session": "local_0a1b",
        "asked": "2026-10-21T09:00:00Z",
        "status": status,
    }
    if belongs_to is not None:
        record["for"] = belongs_to
    return record
