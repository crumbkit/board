"""Fixture boards that run the real `board` binary, which has this plugin built in, with the plugin enabled."""

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


def board_binary() -> Path:
    """The `board` under test: `$BOARD_BIN`, or the workspace's debug build."""
    return Path(os.environ.get("BOARD_BIN", REPOSITORY / "target" / "debug" / "board")).resolve()


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
    """A git repository holding a board, with its own data and cache directories."""

    root: Path
    data: Path
    cache: Path

    def write(self, path: str, text: str) -> None:
        """Write a fixture file at `path`, relative to the board's root. Every test writes through here."""
        target = self.root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text)

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

    def call(self, capability: str, given: dict[str, Any]) -> dict[str, Any]:
        """The output of `board call plans.<capability>`."""
        report = self.json("call", f"plans.{capability}", *flags(given))
        assert report["schema"] == "board/call-report/v2", report
        output: dict[str, Any] = report["data"]["output"]
        return output

    def refusal(self, capability: str, given: dict[str, Any]) -> dict[str, Any]:
        """The failure `board call plans.<capability>` prints when the plugin refuses."""
        ran = self.run("--json", "call", f"plans.{capability}", *flags(given))
        assert ran.returncode != 0, ran.stdout
        failure: dict[str, Any] = json.loads(ran.stdout)
        return failure


@pytest.fixture
def board(tmp_path: Path) -> Board:
    """A board that does not commit, with plans enabled through `board plugin enable`."""
    fixture = Board(root=tmp_path / "board", data=tmp_path / "data", cache=tmp_path / "cache")
    fixture.root.mkdir()
    subprocess.run(["git", "init", "-q"], cwd=fixture.root, check=True)
    fixture.write(".board/board.json", json.dumps({"$type": "board/config/v1", "commit_on_put": False}))
    enabled = fixture.run("plugin", "enable", "plans")
    assert enabled.returncode == 0, enabled.stderr
    return fixture


def question(status: str = "open", asked: str = "2026-10-01T09:00:00Z", **fields: Any) -> str:  # noqa: ANN401 - the record's other fields, as JSON
    """A `plans/question/v1` record."""
    record: dict[str, Any] = {
        "$type": "plans/question/v1",
        "question": "Which way?",
        "session": "local_0a1b",
        "asked": asked,
        "status": status,
        **fields,
    }
    return json.dumps(record)


def decision(**fields: Any) -> str:  # noqa: ANN401 - the record's other fields, as JSON
    """A `plans/decision/v1` record with a reason."""
    record: dict[str, Any] = {
        "$type": "plans/decision/v1",
        "decision": "Do it.",
        "why": "It is needed.",
        "session": "local_0a1b",
        "decided": "2026-10-01T09:00:00Z",
        **fields,
    }
    return json.dumps(record)
