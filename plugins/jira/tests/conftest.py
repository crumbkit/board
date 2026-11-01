"""Fixture boards that run the real `board` binary with this plugin installed, and a stand-in for `acli`.

Board runs the plugin with the PATH the fixture gives it. That PATH holds a
private directory with `uv` and, when a test asks for one, a stub `acli`, then
only `/usr/bin` and `/bin`. The real `acli` is never on it.
"""

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


@dataclass(frozen=True)
class Board:
    """A git repository holding a board, with this plugin installed for it alone."""

    root: Path
    data: Path
    cache: Path
    config: Path
    bin: Path

    def write(self, path: str, text: str) -> None:
        """Write a fixture file at `path`, relative to the board's root."""
        target = self.root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text)

    def read(self, path: str) -> dict[str, Any]:
        """The JSON record at `path`, relative to the board's root."""
        record: dict[str, Any] = json.loads((self.root / path).read_text())
        return record

    def acli(self, script: str) -> None:
        """Put a stub `acli` on the plugin's PATH that runs `script` in sh and logs its arguments."""
        stub = self.bin / "acli"
        stub.write_text(f'#!/bin/sh\necho "$@" >> "{self.bin}/acli.log"\n{script}\n')
        stub.chmod(0o755)

    def acli_calls(self) -> list[str]:
        """Each argument line the stub `acli` was run with."""
        log = self.bin / "acli.log"
        return log.read_text().splitlines() if log.exists() else []

    def run(self, *arguments: str) -> subprocess.CompletedProcess[str]:
        """Run `board` in the board's root with only the variables a user's shell passes."""
        environment = {
            "PATH": os.pathsep.join([str(board_binary().parent), str(self.bin), "/usr/bin", "/bin"]),
            "HOME": os.environ.get("HOME", str(self.root)),
            "XDG_DATA_HOME": str(self.data),
            "XDG_CACHE_HOME": str(self.cache),
            "XDG_CONFIG_HOME": str(self.config),
        }
        return subprocess.run(
            [str(board_binary()), *arguments],
            cwd=self.root,
            env=environment,
            capture_output=True,
            text=True,
            check=False,
        )

    def ref(self, *flags: str) -> tuple[int, dict[str, Any]]:
        """`board --json call jira.ref <flags>`: the exit code, and the output or the failure."""
        ran = self.run("--json", "call", "jira.ref", *flags)
        document: dict[str, Any] = json.loads(ran.stdout)
        return ran.returncode, document["data"]["output"] if ran.returncode == 0 else document


@pytest.fixture
def board(tmp_path: Path) -> Board:
    """A board in which `board plugin enable jira` wrote the schemas, with no `acli` on the plugin's PATH."""
    fixture = Board(
        root=tmp_path / "board",
        data=tmp_path / "data",
        cache=tmp_path / "cache",
        config=tmp_path / "config",
        bin=tmp_path / "bin",
    )
    fixture.root.mkdir()
    fixture.bin.mkdir()
    uv = shutil.which("uv")
    assert uv is not None, "uv is on PATH"
    (fixture.bin / "uv").symlink_to(uv)
    subprocess.run(["git", "init", "-q"], cwd=fixture.root, check=True)
    fixture.write(".board/board.json", json.dumps({"$type": "board/config/v1", "commit_on_put": False}))
    plugins = fixture.data / "board" / "plugins"
    plugins.mkdir(parents=True)
    (plugins / "jira").symlink_to(PLUGIN)
    enabled = fixture.run("plugin", "enable", "jira")
    assert enabled.returncode == 0, enabled.stderr
    return fixture
