"""A personal board that mounts a team board, both real git repositories, with this plugin installed."""

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
# The evals and runs plugins' schemas: the types the records shared in these tests have.
TYPES = [REPOSITORY / "plugins" / name / "types" for name in ("evals", "runs")]
EVAL_SCHEMA = ".board/types/evals/eval/v1.schema.json"
RUN_SCHEMA = ".board/types/runs/run/v1.schema.json"
SUITE_SCHEMA = ".board/types/runs/suite/v1.schema.json"


def board_binary() -> Path:
    """The `board` under test: `$BOARD_BIN`, or the workspace's debug build."""
    return Path(os.environ.get("BOARD_BIN", REPOSITORY / "target" / "debug" / "board")).resolve()


def git(root: Path, *arguments: str) -> str:
    """Run git in `root` and return what it printed, stripped."""
    ran = subprocess.run(["git", *arguments], cwd=root, capture_output=True, text=True, check=True)
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
class Boards:
    """`personal`, which mounts `team` at `/team`, and the plugin directory and cache their runs use."""

    personal: Path
    team: Path
    data: Path
    cache: Path

    def write(self, root: Path, path: str, text: str) -> None:
        """Write a fixture file at `path` under `root`. Every test writes through here."""
        target = root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text)

    def run(self, root: Path, *arguments: str, stdin: str = "") -> subprocess.CompletedProcess[str]:
        """Run `board` in `root`, with only the variables a user's shell would pass it."""
        uv = shutil.which("uv")
        assert uv is not None, "uv is on PATH"
        path = os.pathsep.join([str(board_binary().parent), str(Path(uv).parent), "/usr/bin", "/bin"])
        environment = {
            "PATH": path,
            "HOME": os.environ.get("HOME", str(root)),
            "XDG_DATA_HOME": str(self.data),
            "XDG_CACHE_HOME": str(self.cache),
            "XDG_CONFIG_HOME": str(self.cache.parent / "config"),
        }
        return subprocess.run(
            [str(board_binary()), *arguments],
            cwd=root,
            env=environment,
            input=stdin,
            capture_output=True,
            text=True,
            check=False,
        )

    def share(self, **given: Any) -> tuple[int, dict[str, Any]]:  # noqa: ANN401 - the call's input, as JSON
        """`board --json call share.to` from the personal board: its exit code and its document."""
        ran = self.run(self.personal, "--json", "call", "share.to", *flags({"to": "/team", **given}))
        document: dict[str, Any] = json.loads(ran.stdout)
        return ran.returncode, document

    def publish(self) -> str:
        """Commit everything in the team board and move the personal board's pin to that commit."""
        head = commit(self.team)
        moved = self.run(self.personal, "mount", "update", "/team")
        assert moved.returncode == 0, moved.stdout
        return head

    def pin(self) -> str:
        """The commit the personal board's mount of the team board reads."""
        pointer: dict[str, str] = json.loads((self.personal / "team.json").read_text())
        return pointer["commit"]


def repository(root: Path) -> None:
    """A git repository on `main` whose board commits each put, with an identity of its own."""
    root.mkdir()
    git(root, "init", "-q", "-b", "main")
    git(root, "config", "user.name", "t")
    git(root, "config", "user.email", "t@t")
    (root / ".board").mkdir()
    (root / ".board" / "board.json").write_text(json.dumps({"$type": "board/config/v1", "commit_on_put": True}))


def commit(root: Path) -> str:
    """Commit everything in `root` and return the commit."""
    git(root, "add", ".")
    git(root, "commit", "-qm", "fixture")
    return git(root, "rev-parse", "HEAD")


def schemas(root: Path) -> None:
    """Give the board at `root` the evals and runs plugins' schemas."""
    for types in TYPES:
        for schema in types.rglob("*.schema.json"):
            target = root / ".board" / "types" / schema.relative_to(types)
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(schema.read_text())


@pytest.fixture
def boards(tmp_path: Path) -> Boards:
    """A personal board enabling share and holding the evals and runs schemas, which mounts an empty team board."""
    fixture = Boards(
        personal=tmp_path / "personal", team=tmp_path / "team", data=tmp_path / "data", cache=tmp_path / "cache"
    )
    repository(fixture.team)
    head = commit(fixture.team)
    repository(fixture.personal)
    config = {"$type": "board/config/v1", "commit_on_put": True, "plugins": ["share"]}
    fixture.write(fixture.personal, ".board/board.json", json.dumps(config))
    schemas(fixture.personal)
    pointer = {"$type": "board/mount/v1", "repo": str(fixture.team), "follow": "main", "commit": head}
    fixture.write(fixture.personal, "team.json", json.dumps(pointer))
    commit(fixture.personal)
    plugins = fixture.data / "board" / "plugins"
    plugins.mkdir(parents=True)
    (plugins / "share").symlink_to(PLUGIN)
    return fixture


EVAL: dict[str, Any] = {
    "$type": "evals/eval/v1",
    "question": "Does gix resolve a ref faster than git2?",
    "subjects": ["gix", "git2"],
    "suites": ["/suites/x"],
    "runs": ["/suites/x/runs/1"],
    "status": "concluded",
    "verdict": "supported",
}
SUITE: dict[str, Any] = {
    "$type": "runs/suite/v1",
    "subjects": ["gix", "git2"],
    "method": "Resolve 1,000 refs and compare wall time.",
}
RUN: dict[str, Any] = {
    "$type": "runs/run/v1",
    "suite": "/suites/x",
    "by": "claude",
    "started": "2026-10-01T09:00:00Z",
    "scope": "full",
    "results": [{"case": "cold", "outcome": "pass", "evidence": ["/evals/x/evidence/log.txt"]}],
}


def experiment(boards: Boards, cases_file: str = "/notes/cases.md", **fields: Any) -> None:  # noqa: ANN401 - the eval's other fields, as JSON
    """Write `/evals/x`, its suite and the cases file it names, its run and that run's evidence."""
    boards.write(boards.personal, "evals/x.json", json.dumps({**EVAL, **fields}))
    boards.write(boards.personal, "suites/x.json", json.dumps({**SUITE, "cases_file": cases_file}))
    boards.write(boards.personal, "suites/x/runs/1.json", json.dumps(RUN))
    boards.write(boards.personal, "evals/x/evidence/log.txt", "resolved 1000 refs in 41 ms\n")
    boards.write(boards.personal, "notes/cases.md", "# Cases\n\n- cold\n")
