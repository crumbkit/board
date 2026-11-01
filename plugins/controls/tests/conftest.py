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

    def run(self, *arguments: str, stdin: str = "", cwd: Path | None = None) -> subprocess.CompletedProcess[str]:
        """Run `board` in `cwd`, the board's root by default, with only the variables a user's shell would pass it."""
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
        }
        return subprocess.run(
            [str(board_binary()), *arguments],
            cwd=cwd or self.root,
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

    def call_json(self, capability: str, given: dict[str, Any], cwd: Path | None = None) -> dict[str, Any]:
        """Run `board --json -C <root> call controls.<capability>` in `cwd`, `given` on stdin, and read its document."""
        ran = self.run(
            "--json", "-C", str(self.root), "call", f"controls.{capability}", stdin=json.dumps(given), cwd=cwd
        )
        document: dict[str, Any] = json.loads(ran.stdout)
        return document


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
    return json.dumps({"$type": "board/mount/v1", "repo": str(root), "follow": "main", "commit": commit(root, files)})


def repo(remote: str, root: Path | None = None, files: dict[str, str] | None = None) -> str:
    """A repo record for `remote`; with `root`, mounting `files` committed to a new repository there."""
    record: dict[str, Any] = {"$type": "board/repo/v1", "remote": remote}
    if root is not None:
        record["checkout"] = {"repo": str(root), "follow": "main", "commit": commit(root, files or {}, remote)}
    return json.dumps(record)


def commit(root: Path, files: dict[str, str], origin: str | None = None) -> str:
    """Commit `files` to a new repository at `root`, with `origin` as its remote when given, and return the commit."""
    root.mkdir(parents=True, exist_ok=True)
    for path, text in files.items():
        (root / path).parent.mkdir(parents=True, exist_ok=True)
        (root / path).write_text(text)
    git = ["git", "-c", "user.name=Test", "-c", "user.email=test@example.com"]
    subprocess.run(["git", "init", "-q", "-b", "main"], cwd=root, check=True)
    subprocess.run([*git, "add", "."], cwd=root, check=True)
    subprocess.run([*git, "commit", "-qm", "library", "--allow-empty"], cwd=root, check=True)
    if origin is not None:
        subprocess.run(["git", "remote", "add", "origin", f"https://github.com/{origin}.git"], cwd=root, check=True)
    return subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=root, check=True, capture_output=True, text=True
    ).stdout.strip()
