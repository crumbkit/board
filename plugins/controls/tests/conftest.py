"""Fixture boards that run the real `board` binary with this plugin installed and enabled."""

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
    """A git repository holding a board, with this plugin installed for it alone."""

    root: Path
    data: Path
    cache: Path
    # A directory searched before Board's own on the PATH the plugin sees, such as one holding a wrapper.
    first: Path | None = None

    def write(self, path: str, text: str) -> None:
        """Write a fixture file at `path`, relative to the board's root."""
        target = self.root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text)

    def run(
        self, *arguments: str, stdin: str = "", extra: dict[str, str] | None = None
    ) -> subprocess.CompletedProcess[str]:
        """Run `board` in the board's root, with only the variables a user's shell would pass it, and `extra`."""
        uv = shutil.which("uv")
        assert uv is not None, "uv is on PATH"
        directories = [board_binary().parent, Path(uv).parent, Path("/usr/bin"), Path("/bin")]
        if self.first is not None:
            directories.insert(0, self.first)
        path = os.pathsep.join(str(directory) for directory in directories)
        environment = {
            "PATH": path,
            "HOME": os.environ.get("HOME", str(self.root)),
            "XDG_DATA_HOME": str(self.data),
            "XDG_CACHE_HOME": str(self.cache),
            "XDG_CONFIG_HOME": str(self.cache.parent / "config"),
        } | (extra or {})
        return subprocess.run(
            [str(board_binary()), *arguments],
            cwd=self.root,
            env=environment,
            input=stdin,
            capture_output=True,
            text=True,
            check=False,
        )

    def call(self, capability: str, given: dict[str, Any]) -> dict[str, Any]:
        """Run `board --json call controls.<capability>` and read its one document."""
        ran = self.run("--json", "call", f"controls.{capability}", *flags(given))
        document: dict[str, Any] = json.loads(ran.stdout)
        return document


# A stub in place of a model: it keeps the prompt, counts its calls and prints `answer.txt` beside it.
AGENT = """#!/bin/sh
here="$(dirname "$0")"
cat > "$here/prompt.txt"
echo call >> "$here/calls.txt"
cat "$here/answer.txt"
"""


@dataclass(frozen=True)
class Agent:
    """The stub agent in `directory`, which `evaluate` starts through BOARD_CONTROLS_AGENT."""

    directory: Path

    @property
    def argv(self) -> list[str]:
        """The agent's argv, as BOARD_CONTROLS_AGENT gives it."""
        return [str(self.directory / "agent")]

    def answers(self, findings: list[dict[str, Any]]) -> None:
        """Make the agent answer with some prose and then `findings`."""
        (self.directory / "answer.txt").write_text(f"Read it.\n{json.dumps({'findings': findings})}\n")

    def calls(self) -> int:
        """How many times the agent ran."""
        calls = self.directory / "calls.txt"
        return len(calls.read_text().splitlines()) if calls.exists() else 0

    def prompt(self) -> str:
        """What the agent last read."""
        return (self.directory / "prompt.txt").read_text()


@pytest.fixture
def agent(tmp_path: Path) -> Agent:
    """A stub agent that answers with no findings until told otherwise."""
    stub = Agent(tmp_path / "agent")
    stub.directory.mkdir()
    script = stub.directory / "agent"
    script.write_text(AGENT)
    script.chmod(0o755)
    stub.answers([])
    return stub


@pytest.fixture
def board(tmp_path: Path) -> Board:
    """A board that does not commit, with controls installed and enabled through `board plugin enable`."""
    fixture = Board(root=tmp_path / "board", data=tmp_path / "data", cache=tmp_path / "cache")
    fixture.root.mkdir()
    subprocess.run(["git", "init", "-q"], cwd=fixture.root, check=True)
    fixture.write(".board/board.json", json.dumps({"$type": "board/config/v1", "commit_on_put": False}))
    plugins = fixture.data / "board" / "plugins"
    plugins.mkdir(parents=True)
    (plugins / "controls").symlink_to(PLUGIN)
    enabled = fixture.run("plugin", "enable", "controls")
    assert enabled.returncode == 0, enabled.stderr
    return fixture


def control(requirement: str, **fields: Any) -> str:  # noqa: ANN401 - the record's other fields, as JSON
    """A `controls/control/v1` record."""
    return json.dumps({"$type": "controls/control/v1", "requirement": requirement, "purpose": "A test.", **fields})


def library(root: Path, files: dict[str, str]) -> str:
    """Commit `files` to a new repository at `root`, and return a mount pointer to that commit."""
    for path, text in files.items():
        (root / path).parent.mkdir(parents=True, exist_ok=True)
        (root / path).write_text(text)
    git = ["git", "-c", "user.name=Test", "-c", "user.email=test@example.com"]
    subprocess.run(["git", "init", "-q", "-b", "main"], cwd=root, check=True)
    subprocess.run([*git, "add", "."], cwd=root, check=True)
    subprocess.run([*git, "commit", "-qm", "library"], cwd=root, check=True)
    commit = subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=root, check=True, capture_output=True, text=True
    ).stdout.strip()
    return json.dumps({"$type": "board/mount/v1", "repo": str(root), "follow": "main", "commit": commit})
