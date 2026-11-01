"""Fixture boards that run the real `board` with this plugin, changes and controls installed, and plans and evals."""

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
INSTALLED = ("inflight", "changes", "controls")
BUILT_IN = ("plans", "evals")
HEAD = "a3f9c0de1234567890abcdef1234567890abcdef"
BASE = "0123456789abcdef0123456789abcdef01234567"


def ref(**more: Any) -> dict[str, Any]:  # noqa: ANN401 - more schema keywords
    """A reference's schema."""
    return {"type": "string", "format": "board-ref", **more}


# Types other plugins own, which `now` or `changes.next` read. The fixture keeps a loose schema of each, with the
# references they follow, so Board indexes them.
FOREIGN_TYPES: dict[str, dict[str, Any]] = {
    "runs/suite/v1": {},
    "runs/run/v1": {"agent": ref()},
    "runs/agent/v1": {"works_on": ref()},
    "review/brief/v1": {"for": ref(), "repo": ref()},
    "review/report/v1": {"brief": ref()},
    "artifacts/artifact/v1": {},
    "artifacts/version/v1": {"for": {"type": "array", "items": ref()}},
}


def board_binary() -> Path:
    """The `board` under test: `$BOARD_BIN`, or the workspace's debug build."""
    return Path(os.environ.get("BOARD_BIN", REPOSITORY / "target" / "debug" / "board")).resolve()


def flags(given: dict[str, Any]) -> list[str]:
    """`given` as the flags of `board call`."""
    parts: list[str] = []
    for name, value in given.items():
        parts += [f"--{name}", str(value)]
    return parts


@dataclass(frozen=True)
class Board:
    """A git repository holding a board, with the plugins installed for it alone."""

    root: Path
    data: Path
    cache: Path

    def write(self, path: str, text: str) -> None:
        """Write a fixture file at `path`, relative to the board's root. Every test writes through here."""
        target = self.root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text)

    def run(self, *arguments: str, stdin: str = "") -> subprocess.CompletedProcess[str]:
        """Run `board` in the board's root with only the variables a user's shell would pass it."""
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
        return subprocess.run(
            [str(board_binary()), *arguments],
            cwd=self.root,
            env=environment,
            input=stdin,
            capture_output=True,
            text=True,
            check=False,
        )

    def json(self, *arguments: str) -> dict[str, Any]:
        """Run `board --json` and read its one document."""
        document: dict[str, Any] = json.loads(self.run("--json", *arguments).stdout)
        return document

    def call(self, capability: str, given: dict[str, Any]) -> dict[str, Any]:
        """The output of `board call <capability>`, given as flags."""
        report = self.json("call", capability, *flags(given))
        assert report.get("schema") == "board/call-report/v2", report
        output: dict[str, Any] = report["data"]["output"]
        return output

    def now(self, **given: Any) -> dict[str, Any]:  # noqa: ANN401 - the call's flags
        """The output of `board call inflight.now` with `given` as its flags."""
        return self.call("inflight.now", given)

    def refusal(self, capability: str, given: dict[str, Any]) -> dict[str, Any]:
        """The failure `board call <capability>` prints when it is refused."""
        ran = self.run("--json", "call", capability, *flags(given))
        assert ran.returncode != 0, ran.stdout
        failure: dict[str, Any] = json.loads(ran.stdout)
        return failure


@pytest.fixture(scope="session")
def installed() -> None:
    """Each installed plugin's environment, which its manifest runs directly."""
    for name in INSTALLED:
        subprocess.run(["uv", "sync", "--locked", "--quiet"], cwd=REPOSITORY / "plugins" / name, check=True)


@pytest.fixture
def board(tmp_path: Path, installed: None) -> Board:  # noqa: ARG001 - the fixture installs the plugins
    """A board that does not commit, with inflight, changes, controls, plans and evals enabled."""
    fixture = Board(root=tmp_path / "a board", data=tmp_path / "data", cache=tmp_path / "cache")
    fixture.root.mkdir()
    subprocess.run(["git", "init", "-q"], cwd=fixture.root, check=True)
    fixture.write(".board/board.json", json.dumps({"$type": "board/config/v1", "commit_on_put": False}))
    for name, properties in FOREIGN_TYPES.items():
        fixture.write(f".board/types/{name}.schema.json", json.dumps({"type": "object", "properties": properties}))
    plugins = fixture.data / "board" / "plugins"
    plugins.mkdir(parents=True)
    for name in INSTALLED:
        (plugins / name).symlink_to(REPOSITORY / "plugins" / name)
    for name in (*INSTALLED, *BUILT_IN):
        enabled = fixture.run("plugin", "enable", name)
        assert enabled.returncode == 0, enabled.stderr
    fixture.call("changes.seed", {})
    fixture.write("suites/checks.json", record("runs/suite/v1", subjects=["app"], method="Run it."))
    fixture.write("repos/app.json", record("board/repo/v1", remote="example/app"))
    return fixture


def record(type_name: str, **fields: Any) -> str:  # noqa: ANN401 - the record's other fields, as JSON
    """A record of `type_name` holding `fields`."""
    return json.dumps({"$type": type_name, **fields})


def change(board: Board, name: str, route: str = "owner", session: str | None = None) -> str:
    """A change at /changes/<name> to /repos/app on branch <name>, claimed by `session` when given; its path."""
    fields = {
        "title": f"Change {name}",
        "repo": "/repos/app",
        "branch": name,
        "workflow": "/workflows/personal",
        "suite": "/suites/checks",
        "route": route,
    }
    board.write(f"changes/{name}.json", record("changes/change/v2", **fields))
    if session is not None:
        claim = record("changes/claim/v1", session=session, worktree=f"/work/{name}")
        board.write(f"changes/{name}/claims/20261101T080000000000Z.json", claim)
    return f"/changes/{name}"


def planned(board: Board, name: str) -> None:
    """A plan for the change at /changes/<name>, which uses nothing."""
    board.write(f"changes/{name}/plan.md", "# Plan\n\n## Outcome\n\nIt works.\n")
    plan = record("changes/plan/v1", text=f"/changes/{name}/plan.md", uses=[])
    board.write(f"changes/{name}/plans/20261101T080000000000Z.json", plan)


def checked(board: Board, name: str) -> None:
    """A passing full run of a clean tree at HEAD, recorded as the change's newest check."""
    run = record(
        "runs/run/v1",
        suite="/suites/checks",
        by="t",
        started="2026-11-01T08:00:00Z",
        scope="full",
        dirty=False,
        versions={"app": HEAD},
        results=[],
    )
    board.write(f"suites/checks/runs/{name}.json", run)
    board.write(
        f"changes/{name}/checks/20261101T080000000000Z.json",
        record("changes/check/v1", run=f"/suites/checks/runs/{name}"),
    )


def agent(board: Board, name: str, started: str, works_on: str | None = None, session: str = "s1") -> str:
    """An agent record at /agents/<name> with its prompt beside it; its path."""
    fields: dict[str, Any] = {"session": session, "cwd": "/work", "started": started}
    if works_on is not None:
        fields["works_on"] = works_on
    board.write(f"agents/{name}.json", record("runs/agent/v1", **fields))
    board.write(f"agents/{name}.md", "Do the work.\n")
    return f"/agents/{name}"


def stopped(board: Board, path: str) -> None:
    """A run that names the agent at `path`, which says it stopped."""
    run = record("runs/run/v1", suite="/suites/checks", by="t", started="2026-11-01T08:00:00Z", agent=path)
    board.write(f"suites/checks/runs/{path.rsplit('/', 1)[-1]}-stop.json", run)
