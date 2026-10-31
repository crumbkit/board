"""Fixture boards that run the real `board` binary, which has this plugin built in, with runs installed."""

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
TYPES = REPOSITORY / "crates" / "board-plugin-evals" / "types"
SCHEMAS = TYPES / "evals"
# The runs plugin, whose suites and runs an eval refers to.
RUNS = REPOSITORY / "plugins" / "runs"
EVAL_TYPE = "evals/eval/v1"


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
    """A git repository holding a board, with the runs plugin installed for it alone."""

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
        """The output of `board call evals.<capability>`."""
        report = self.json("call", f"evals.{capability}", *flags(given))
        assert report["schema"] == "board/call-report/v2", report
        output: dict[str, Any] = report["data"]["output"]
        return output

    def paths(self, capability: str, given: dict[str, Any]) -> list[str]:
        """The paths of the evals a capability lists, in its order."""
        return [item["path"] for item in self.call(capability, given)["evals"]]


@pytest.fixture
def board(tmp_path: Path) -> Board:
    """A board that does not commit, with runs installed and both plugins enabled through `board plugin enable`."""
    fixture = Board(root=tmp_path / "board", data=tmp_path / "data", cache=tmp_path / "cache")
    fixture.root.mkdir()
    subprocess.run(["git", "init", "-q"], cwd=fixture.root, check=True)
    fixture.write(".board/board.json", json.dumps({"$type": "board/config/v1", "commit_on_put": False}))
    plugins = fixture.data / "board" / "plugins"
    plugins.mkdir(parents=True)
    (plugins / "runs").symlink_to(RUNS)
    for name in ("evals", "runs"):
        enabled = fixture.run("plugin", "enable", name)
        assert enabled.returncode == 0, enabled.stderr
    return fixture


def experiment(question: str, subjects: list[str] | None = None, status: str = "planned", **fields: Any) -> str:  # noqa: ANN401 - the record's other fields, as JSON
    """An `evals/eval/v1` record."""
    record: dict[str, Any] = {
        "$type": EVAL_TYPE,
        "question": question,
        "subjects": ["gix"] if subjects is None else subjects,
        "status": status,
        **fields,
    }
    return json.dumps(record)


def suite() -> str:
    """A `runs/suite/v1` record."""
    return json.dumps({"$type": "runs/suite/v1", "subjects": ["gix"], "method": "Run it."})


def run(suite_path: str) -> str:
    """A `runs/run/v1` record of `suite_path` with one passing result."""
    record: dict[str, Any] = {
        "$type": "runs/run/v1",
        "suite": suite_path,
        "by": "claude",
        "started": "2026-10-01T09:00:00Z",
        "scope": "full",
        "results": [{"case": "one", "outcome": "pass"}],
    }
    return json.dumps(record)


def repository(parent: Path, name: str, records: dict[str, str]) -> tuple[Path, str]:
    """A board in a git repository that enables evals and runs, holding `records` by path, and its one commit."""
    root = parent / name
    for types in (TYPES, RUNS / "types"):
        for schema in types.rglob("*.schema.json"):
            target = root / ".board" / "types" / schema.relative_to(types)
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(schema.read_text())
    config = {"$type": "board/config/v1", "plugins": ["evals", "runs"]}
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
