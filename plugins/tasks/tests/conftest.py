"""Fixture boards that run the real `board` binary with this plugin installed."""

import json
import os
import shutil
import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import pytest

PLUGIN = Path(__file__).resolve().parents[1]
FIXTURES = Path(__file__).resolve().parent / "fixtures"
REPOSITORY = PLUGIN.parents[1]
SCHEMA = PLUGIN / "types" / "tasks" / "task" / "v1.schema.json"


def board_binary() -> Path:
    """The `board` under test: `$BOARD_BIN`, or the workspace's debug build."""
    return Path(os.environ.get("BOARD_BIN", REPOSITORY / "target" / "debug" / "board")).resolve()


@dataclass(frozen=True)
class Board:
    """A git repository holding a board, with plugins installed for it alone."""

    root: Path
    data: Path
    cache: Path

    def write(self, path: str, text: str) -> None:
        """Write a fixture file at `path`, relative to the board's root. Every test writes through here."""
        target = self.root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text)

    def enable(self, *plugins: str) -> None:
        """Make `.board/board.json` enable `plugins`, without committing a put."""
        config = {"$type": "board/config/v1", "commit_on_put": False, "plugins": list(plugins)}
        self.write(".board/board.json", json.dumps(config))

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


@pytest.fixture
def board(tmp_path: Path) -> Board:
    """A board enabling digest and tasks, which `board plugin enable` gave the task schema."""
    fixture = Board(root=tmp_path / "board", data=tmp_path / "data", cache=tmp_path / "cache")
    fixture.root.mkdir()
    subprocess.run(["git", "init", "-q"], cwd=fixture.root, check=True)
    fixture.enable("digest")
    plugins = fixture.data / "board" / "plugins"
    plugins.mkdir(parents=True)
    (plugins / "tasks").symlink_to(PLUGIN)
    (plugins / "digest").symlink_to(FIXTURES / "digest")
    enabled = fixture.run("plugin", "enable", "tasks")
    assert enabled.returncode == 0, enabled.stderr
    return fixture


def task(title: str, status: str = "open", due: str | None = None, **fields: Any) -> str:  # noqa: ANN401 - the record's other fields, as JSON
    """A `tasks/task/v1` record."""
    record: dict[str, Any] = {"$type": "tasks/task/v1", "title": title, "status": status, **fields}
    if due is not None:
        record["due"] = due
    return json.dumps(record)
