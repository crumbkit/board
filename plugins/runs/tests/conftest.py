"""Fixture boards that run the real `board` binary, which has this plugin built in."""

import json
import os
import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import pytest

PLUGIN = Path(__file__).resolve().parents[1]
REPOSITORY = PLUGIN.parents[1]
# The schemas the plugin ships, kept in its crate.
TYPES = REPOSITORY / "crates" / "board-plugin-runs" / "types"
SCHEMAS = TYPES / "runs"
SUITE_TYPE = "runs/suite/v1"
RUN_TYPE = "runs/run/v1"


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
    """A git repository holding a board, run with a data directory of its own."""

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
        path = os.pathsep.join([str(board_binary().parent), "/usr/bin", "/bin"])
        environment = {
            "PATH": path,
            "HOME": os.environ.get("HOME", str(self.root)),
            "XDG_DATA_HOME": str(self.data),
            "XDG_CACHE_HOME": str(self.cache),
            "XDG_CONFIG_HOME": str(self.cache.parent / "config"),
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
        """The output of `board call runs.<capability>`."""
        report = self.json("call", f"runs.{capability}", *flags(given))
        assert report["schema"] == "board/call-report/v2", report
        output: dict[str, Any] = report["data"]["output"]
        return output

    def refusal(self, capability: str, given: dict[str, Any]) -> dict[str, Any]:
        """The failure `board call runs.<capability>` prints when the plugin refuses."""
        ran = self.run("--json", "call", f"runs.{capability}", *flags(given))
        assert ran.returncode != 0, ran.stdout
        failure: dict[str, Any] = json.loads(ran.stdout)
        return failure


@pytest.fixture
def board(tmp_path: Path) -> Board:
    """A board that does not commit, with runs enabled through `board plugin enable`."""
    fixture = Board(root=tmp_path / "board", data=tmp_path / "data", cache=tmp_path / "cache")
    fixture.root.mkdir()
    subprocess.run(["git", "init", "-q"], cwd=fixture.root, check=True)
    fixture.write(".board/board.json", json.dumps({"$type": "board/config/v1", "commit_on_put": False}))
    enabled = fixture.run("plugin", "enable", "runs")
    assert enabled.returncode == 0, enabled.stderr
    return fixture


def suite(**fields: Any) -> str:  # noqa: ANN401 - the record's other fields, as JSON
    """A `runs/suite/v1` record."""
    record: dict[str, Any] = {"$type": SUITE_TYPE, "subjects": ["board"], "method": "Run it.", **fields}
    return json.dumps(record)


def run(suite_path: str, started: str = "2026-10-01T09:00:00Z", **fields: Any) -> str:  # noqa: ANN401 - the record's other fields, as JSON
    """A `runs/run/v1` record of a full run of a clean revision that passed."""
    record: dict[str, Any] = {
        "$type": RUN_TYPE,
        "suite": suite_path,
        "by": "claude",
        "started": started,
        "scope": "full",
        "dirty": False,
        "results": [],
        **fields,
    }
    return json.dumps(record)


def repository(parent: Path, name: str, records: dict[str, str]) -> tuple[Path, str]:
    """A board in a git repository that enables runs, holding `records` by path, and its one commit."""
    root = parent / name
    for schema in SCHEMAS.rglob("*.schema.json"):
        target = root / ".board" / "types" / schema.relative_to(TYPES)
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(schema.read_text())
    config = {"$type": "board/config/v1", "plugins": ["runs"]}
    (root / ".board" / "board.json").write_text(json.dumps(config))
    for path, text in records.items():
        (root / path).parent.mkdir(parents=True, exist_ok=True)
        (root / path).write_text(text)
    identity = ["-c", "user.name=t", "-c", "user.email=t@t"]
    for command in (["init", "-q"], ["add", "."], [*identity, "commit", "-qm", "t"]):
        subprocess.run(["git", *command], cwd=root, check=True)
    head = subprocess.run(["git", "rev-parse", "HEAD"], cwd=root, capture_output=True, text=True, check=True)
    return root, head.stdout.strip()


def mount(board: Board, path: str, repo: Path, commit: str) -> None:
    """Mount `repo` at `commit` at `path` in `board`, read-only, following `HEAD`."""
    pointer = {"$type": "board/mount/v1", "repo": str(repo), "follow": "HEAD", "commit": commit}
    board.write(path, json.dumps(pointer))
