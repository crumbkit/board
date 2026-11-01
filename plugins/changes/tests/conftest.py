"""Fixture boards that run the real `board` binary with this plugin installed and enabled."""

import json
import os
import shutil
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import pytest

PLUGIN = Path(__file__).resolve().parents[1]
REPOSITORY = PLUGIN.parents[1]
HEAD = "a3f9c0de1234567890abcdef1234567890abcdef"
# Types other plugins own, which a change references. The fixture board keeps a loose schema of its own for each.
FOREIGN_TYPES = (
    "tasks/task/v2",
    "runs/suite/v1",
    "runs/run/v1",
    "plans/decision/v1",
    "jira/issue/v1",
    "github/issue/v1",
    "github/pull/v1",
)
REVIEW_TYPE = "review/report/v1"
# A review report as the review plugin's design defines it: the range and patch ids covered, findings and a verdict.
REVIEW_SCHEMA = {
    "type": "object",
    "required": ["range", "patch_ids", "findings", "verdict", "by"],
    "properties": {
        "brief": {"type": "string", "format": "board-ref"},
        "range": {"type": "string", "pattern": "^[0-9a-f]{7,40}\\.\\.[0-9a-f]{7,40}$"},
        "patch_ids": {"type": "array", "items": {"type": "string"}},
        "findings": {
            "type": "array",
            "items": {
                "type": "object",
                "required": ["severity", "text"],
                "properties": {
                    "severity": {"enum": ["critical", "high", "medium", "low"]},
                    "file": {"type": "string"},
                    "line": {"type": "integer"},
                    "text": {"type": "string"},
                },
            },
        },
        "verdict": {"enum": ["approve", "changes", "comment"]},
        "by": {"type": "string"},
        "cost": {"type": "object"},
        "source": {"type": "string"},
        "follows": {"type": "string", "format": "board-ref"},
    },
}
BASE = "0123456789abcdef0123456789abcdef01234567"


def board_binary() -> Path:
    """The `board` under test: `$BOARD_BIN`, or the workspace's debug build."""
    return Path(os.environ.get("BOARD_BIN", REPOSITORY / "target" / "debug" / "board")).resolve()


def git(root: Path, *arguments: str) -> str:
    """Run git in `root` with an identity of its own and return its output."""
    identity = ["-c", "user.name=t", "-c", "user.email=t@t"]
    ran = subprocess.run(["git", *identity, *arguments], cwd=root, capture_output=True, text=True, check=True)
    return ran.stdout.strip()


def flags(given: dict[str, Any]) -> list[str]:
    """`given` as the flags of `board call`; a value no flag can give, such as None or a list, is refused."""
    parts: list[str] = []
    for name, value in given.items():
        if not isinstance(value, str | int | float):
            message = f"{name} is {value!r}, which no flag can give"
            raise TypeError(message)
        parts += [f"--{name}", json.dumps(value) if isinstance(value, bool) else str(value)]
    return parts


@dataclass(frozen=True)
class Board:
    """A git repository holding a board, with this plugin installed for it alone."""

    root: Path
    data: Path
    cache: Path

    def write(self, path: str, text: str) -> None:
        """Write a fixture file at `path`, relative to the board's root. Every test writes through here."""
        target = self.root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text)

    def read(self, path: str) -> dict[str, Any]:
        """The JSON record at `path`, relative to the board's root."""
        record: dict[str, Any] = json.loads((self.root / path).read_text())
        return record

    def run(self, *arguments: str, stdin: str = "") -> subprocess.CompletedProcess[str]:
        """Run `board` in the board's root, with only the variables a user's shell would pass it."""
        uv = shutil.which("uv")
        assert uv is not None, "uv is on PATH"
        path = os.pathsep.join([str(board_binary().parent), str(Path(uv).parent), "/usr/bin", "/bin"])
        environment = {
            "PATH": path,
            "HOME": os.environ.get("HOME", str(self.root)),
            "XDG_DATA_HOME": str(self.data),
            "XDG_CACHE_HOME": str(self.cache),
        }
        return subprocess.run(
            [str(board_binary()), *arguments],
            cwd=self.root,
            env=environment,
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

    def call(self, capability: str, given: dict[str, Any], *, stdin: bool = False) -> dict[str, Any]:
        """The output of `board call changes.<capability>`, given as flags or, with `stdin`, as JSON on stdin."""
        report = self.json(
            "call", f"changes.{capability}", *self.as_flags(given, stdin=stdin), stdin=self.piped(given, stdin=stdin)
        )
        assert report.get("schema") == "board/call-report/v2", report
        output: dict[str, Any] = report["data"]["output"]
        return output

    def refusal(self, capability: str, given: dict[str, Any], *, stdin: bool = False) -> dict[str, Any]:
        """The failure `board call changes.<capability>` prints when the plugin refuses."""
        ran = self.run(
            "--json",
            "call",
            f"changes.{capability}",
            *self.as_flags(given, stdin=stdin),
            stdin=self.piped(given, stdin=stdin),
        )
        assert ran.returncode != 0, ran.stdout
        failure: dict[str, Any] = json.loads(ran.stdout)
        return failure

    @staticmethod
    def as_flags(given: dict[str, Any], *, stdin: bool) -> list[str]:
        """`given` as flags, or no flags when it goes on stdin."""
        return [] if stdin else flags(given)

    @staticmethod
    def piped(given: dict[str, Any], *, stdin: bool) -> str:
        """`given` as JSON when it goes on stdin, else nothing."""
        return json.dumps(given) if stdin else ""

    def plugin(self, capability: str, given: dict[str, Any]) -> tuple[int, dict[str, Any]]:
        """Run the plugin once on a call of `capability`, as Board runs it, for a call Board itself would refuse."""
        request = {
            "$type": "board/call/v1",
            "capability": capability,
            "input": given,
            "board": {"root": str(self.root)},
        }
        path = os.pathsep.join([str(board_binary().parent), os.environ.get("PATH", "")])
        ran = subprocess.run(
            [sys.executable, "-m", "board_changes"],
            cwd=self.root,
            env={**os.environ, "PATH": path, "XDG_DATA_HOME": str(self.data), "XDG_CACHE_HOME": str(self.cache)},
            input=json.dumps(request),
            capture_output=True,
            text=True,
            check=False,
        )
        document: dict[str, Any] = json.loads(ran.stdout)
        return ran.returncode, document


@pytest.fixture
def board(tmp_path: Path) -> Board:
    """A board that does not commit, at a root holding a space, with changes and a loose schema for each cited type."""
    fixture = Board(root=tmp_path / "a board", data=tmp_path / "data", cache=tmp_path / "cache")
    fixture.root.mkdir()
    git(fixture.root, "init", "-q")
    fixture.write(".board/board.json", json.dumps({"$type": "board/config/v1", "commit_on_put": False}))
    for name in FOREIGN_TYPES:
        fixture.write(f".board/types/{name}.schema.json", json.dumps({"type": "object"}))
    fixture.write(f".board/types/{REVIEW_TYPE}.schema.json", json.dumps(REVIEW_SCHEMA))
    plugins = fixture.data / "board" / "plugins"
    plugins.mkdir(parents=True)
    (plugins / "changes").symlink_to(PLUGIN)
    enabled = fixture.run("plugin", "enable", "changes")
    assert enabled.returncode == 0, enabled.stderr
    return fixture


def mount_repo(board: Board, files: dict[str, str], at: str = "repos/x") -> str:
    """A git repository holding a board and `files`, mounted at `/<at>`; its logical path."""
    root = board.root.parent / at.replace("/", "-")
    root.mkdir()
    git(root, "init", "-q")
    (root / ".board").mkdir()
    (root / ".board" / "board.json").write_text(json.dumps({"$type": "board/config/v1"}))
    for path, text in files.items():
        (root / path).parent.mkdir(parents=True, exist_ok=True)
        (root / path).write_text(text)
    git(root, "add", ".")
    git(root, "commit", "-qm", "fixture")
    pointer = {"$type": "board/mount/v1", "repo": str(root), "follow": "HEAD", "commit": git(root, "rev-parse", "HEAD")}
    board.write(f"{at}.json", json.dumps(pointer))
    return f"/{at}"


def record(type_name: str, **fields: Any) -> str:  # noqa: ANN401 - the record's other fields, as JSON
    """A record of `type_name` holding `fields`."""
    return json.dumps({"$type": type_name, **fields})


def task(status: str = "open") -> str:
    """A `tasks/task/v2` Markdown task titled "A" in `status`."""
    return f"---\n$type: tasks/task/v2\ntitle: A\nstatus: {status}\n---\n"


def report(head: str = HEAD, verdict: str = "approve", **fields: Any) -> str:  # noqa: ANN401 - other fields
    """A review report of `base..head` with `verdict` and no findings."""
    body: dict[str, Any] = {
        "range": f"{BASE}..{head}",
        "patch_ids": ["p1"],
        "findings": [],
        "verdict": verdict,
        "by": "a reviewer",
    }
    return record(REVIEW_TYPE, **(body | fields))


def change(phase: str = "plan", **fields: Any) -> str:  # noqa: ANN401 - the record's other fields, as JSON
    """An `changes/change/v1` record in `phase`."""
    defaults: dict[str, Any] = {"title": "Let a suite declare a timeout", "mount": "/repos/x", "phase": phase}
    return record("changes/change/v1", **(defaults | fields))


PLAN = "# Timeout\n\n## Problem\n\nSuites hang.\n\n## Outcome\n\nA suite stops.\n\n## Out of scope\n\nRetries.\n"
