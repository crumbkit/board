"""Fixture boards that run the real `board` binary with this plugin and the controls plugin installed and enabled."""

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
CONTROLS = REPOSITORY / "plugins" / "controls"
HEAD = "a3f9c0de1234567890abcdef1234567890abcdef"
BASE = "0123456789abcdef0123456789abcdef01234567"
PATH = "/changes/t"


def ref(**more: Any) -> dict[str, Any]:  # noqa: ANN401 - more schema keywords
    """A reference's schema."""
    return {"type": "string", "format": "board-ref", **more}


# Types other plugins own, which a change's steps read. The fixture keeps a loose schema of each, with the
# references the steps follow, so Board indexes them.
FOREIGN_TYPES: dict[str, dict[str, Any]] = {
    "runs/suite/v1": {},
    "runs/run/v1": {},
    "plans/question/v1": {"for": {"type": "array", "items": ref()}},
    "plans/answer/v1": {"question": ref()},
    "review/brief/v1": {"for": ref(), "repo": ref()},
    "review/report/v1": {"brief": ref()},
    "evals/eval/v1": {},
    "plans/decision/v1": {},
}


def board_binary() -> Path:
    """The `board` under test: `$BOARD_BIN`, or the workspace's debug build."""
    return Path(os.environ.get("BOARD_BIN", REPOSITORY / "target" / "debug" / "board")).resolve()


def git(root: Path, *arguments: str) -> str:
    """Run git in `root` with an identity of its own and return its output."""
    identity = ["-c", "user.name=t", "-c", "user.email=t@t", "-c", "init.defaultBranch=main"]
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
    """A git repository holding a board, with this plugin and controls installed for it alone."""

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

    def listed(self, path: str) -> list[str]:
        """The names of the files in the directory at `path`, relative to the board's root, sorted."""
        directory = self.root / path
        return sorted(child.name for child in directory.iterdir()) if directory.is_dir() else []

    def run(self, *arguments: str, stdin: str = "", cwd: Path | None = None) -> subprocess.CompletedProcess[str]:
        """Run `board` in `cwd`, the board's root by default, with only the variables a user's shell would pass it."""
        uv = shutil.which("uv")
        assert uv is not None, "uv is on PATH"
        path = os.pathsep.join([str(board_binary().parent), str(Path(uv).parent), "/usr/bin", "/bin"])
        environment = {
            "PATH": path,
            "HOME": os.environ.get("HOME", str(self.root)),
            "XDG_DATA_HOME": str(self.data),
            "XDG_CACHE_HOME": str(self.cache),
            "XDG_CONFIG_HOME": str(self.cache / "config"),
        }
        located = [] if cwd is None else ["-C", str(self.root)]
        return subprocess.run(
            [str(board_binary()), *located, *arguments],
            cwd=cwd or self.root,
            env=environment,
            input=stdin,
            capture_output=True,
            text=True,
            check=False,
        )

    def json(self, *arguments: str, stdin: str = "", cwd: Path | None = None) -> dict[str, Any]:
        """Run `board --json` and read its one document."""
        ran = self.run("--json", *arguments, stdin=stdin, cwd=cwd)
        document: dict[str, Any] = json.loads(ran.stdout)
        return document

    def call(
        self, capability: str, given: dict[str, Any], *, stdin: bool = False, cwd: Path | None = None
    ) -> dict[str, Any]:
        """The output of `board call changes.<capability>`, given as flags or, with `stdin`, as JSON on stdin."""
        arguments = ("call", f"changes.{capability}", *([] if stdin else flags(given)))
        report = self.json(*arguments, stdin=json.dumps(given) if stdin else "", cwd=cwd)
        assert report.get("schema") == "board/call-report/v2", report
        output: dict[str, Any] = report["data"]["output"]
        return output

    def refusal(
        self, capability: str, given: dict[str, Any], *, stdin: bool = False, cwd: Path | None = None
    ) -> dict[str, Any]:
        """The failure `board call changes.<capability>` prints when the plugin refuses."""
        arguments = ("--json", "call", f"changes.{capability}", *([] if stdin else flags(given)))
        ran = self.run(*arguments, stdin=json.dumps(given) if stdin else "", cwd=cwd)
        assert ran.returncode != 0, ran.stdout
        failure: dict[str, Any] = json.loads(ran.stdout)
        return failure

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


@pytest.fixture(scope="session")
def controls_installed() -> None:
    """The controls plugin's environment, which its manifest runs directly."""
    subprocess.run(["uv", "sync", "--locked", "--quiet"], cwd=CONTROLS, check=True)


@pytest.fixture
def board(tmp_path: Path, controls_installed: None) -> Board:  # noqa: ARG001 - the fixture installs controls
    """A board that does not commit, at a root holding a space, with changes, controls and a schema per read type."""
    fixture = Board(root=tmp_path / "a board", data=tmp_path / "data", cache=tmp_path / "cache")
    fixture.root.mkdir()
    git(fixture.root, "init", "-q")
    fixture.write(".board/board.json", json.dumps({"$type": "board/config/v1", "commit_on_put": False}))
    for name, properties in FOREIGN_TYPES.items():
        schema = {"type": "object", "properties": properties}
        fixture.write(f".board/types/{name}.schema.json", json.dumps(schema))
    plugins = fixture.data / "board" / "plugins"
    plugins.mkdir(parents=True)
    (plugins / "changes").symlink_to(PLUGIN)
    (plugins / "controls").symlink_to(CONTROLS)
    for plugin in ("changes", "controls"):
        enabled = fixture.run("plugin", "enable", plugin)
        assert enabled.returncode == 0, enabled.stderr
    return fixture


@dataclass(frozen=True)
class Clone:
    """A clone of a repository whose origin is `remote`, on a work branch, and the repository it was cloned from."""

    path: Path
    origin: Path
    remote: str


def clone(board: Board, files: dict[str, str] | None = None, name: str = "app", branch: str = "work") -> Clone:
    """A repository holding `files` and a clone of it, on `branch`, whose origin is `example/<name>` on GitHub."""
    origin = board.root.parent / f"{name}-origin"
    origin.mkdir()
    git(origin, "init", "-q")
    for path, text in (files or {"README.md": "x\n"}).items():
        (origin / path).parent.mkdir(parents=True, exist_ok=True)
        (origin / path).write_text(text)
    git(origin, "add", ".")
    git(origin, "commit", "-qm", "fixture")
    path = board.root.parent / f"{name}-clone"
    git(board.root.parent, "clone", "-q", "--local", str(origin), str(path))
    remote = f"https://github.com/example/{name}.git"
    git(path, "remote", "set-url", "origin", remote)
    if branch != "main":
        git(path, "switch", "-qc", branch)
    return Clone(path, origin, remote)


def mount(board: Board, cloned: Clone, at: str = "repos/app") -> str:
    """A `board/mount/v1` of the clone at `/<at>`, so its files are in the board; its logical path."""
    pointer = {
        "$type": "board/mount/v1",
        "repo": str(cloned.path),
        "follow": "HEAD",
        "commit": git(cloned.path, "rev-parse", "HEAD"),
    }
    board.write(f"{at}.json", json.dumps(pointer))
    return f"/{at}"


def record(type_name: str, **fields: Any) -> str:  # noqa: ANN401 - the record's other fields, as JSON
    """A record of `type_name` holding `fields`."""
    return json.dumps({"$type": type_name, **fields})


WORKFLOW = [
    {"name": "plan", "actor": "agent", "done_by": "plan"},
    {"name": "build", "actor": "agent", "done_by": "check"},
    {"name": "review", "actor": "reviewer", "done_by": "review"},
    {"name": "land", "actor": "owner", "done_by": "landed"},
]


def change(**fields: Any) -> str:  # noqa: ANN401 - the record's other fields, as JSON
    """A `changes/change/v2` record to /repos/app on its work branch, with the personal workflow."""
    defaults: dict[str, Any] = {
        "title": "Let a suite declare a timeout",
        "repo": "/repos/app",
        "branch": "work",
        "workflow": "/workflows/personal",
        "suite": "/suites/checks",
        "route": "owner",
    }
    return record("changes/change/v2", **(defaults | fields))


def setup(board: Board, **fields: Any) -> None:  # noqa: ANN401 - the change's other fields, as JSON
    """The change at /changes/t, the seed workflows, its suite and its repo record without a checkout."""
    board.call("seed", {})
    board.write("suites/checks.json", record("runs/suite/v1", subjects=["app"], method="Run it."))
    board.write("repos/app.json", record("board/repo/v1", remote="example/app"))
    board.write("changes/t.json", change(**fields))


def run(**fields: Any) -> str:  # noqa: ANN401 - the run's other fields, as JSON
    """A passing full run of a clean tree at HEAD, as the suite records it."""
    defaults: dict[str, Any] = {
        "suite": "/suites/checks",
        "scope": "full",
        "dirty": False,
        "versions": {"app": HEAD},
        "results": [],
    }
    return record("runs/run/v1", **(defaults | fields))


def report(head: str = HEAD, verdict: str = "approve", **fields: Any) -> str:  # noqa: ANN401 - other fields
    """A review report of `base..head` with `verdict` and no findings."""
    body: dict[str, Any] = {
        "range": f"{BASE}..{head}",
        "patch_ids": ["p1"],
        "findings": [],
        "verdict": verdict,
        "by": "a reviewer",
    }
    return record("review/report/v1", **(body | fields))


PLAN = "# Timeout\n\n## Problem\n\nSuites hang.\n\n## Outcome\n\nA suite stops.\n\n## Out of scope\n\nRetries.\n"
