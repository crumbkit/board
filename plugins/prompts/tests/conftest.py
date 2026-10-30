"""Fixture boards that run the real `board` binary with this plugin installed, and notes repos mounted in them."""

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
SCHEMA = PLUGIN / "types" / "prompts" / "prompt" / "v1.schema.json"
NOTES_SCHEMA = PLUGIN.parent / "notes" / "types" / "notes" / "repo" / "v1.schema.json"


def board_binary() -> Path:
    """The `board` under test: `$BOARD_BIN`, or the workspace's debug build."""
    return Path(os.environ.get("BOARD_BIN", REPOSITORY / "target" / "debug" / "board")).resolve()


@dataclass(frozen=True)
class Board:
    """A git repository holding a board, with plugins installed for it alone."""

    root: Path
    data: Path
    cache: Path
    config: Path

    def write(self, path: str, text: str) -> None:
        """Write a fixture file at `path`, relative to the board's root. Every test writes through here."""
        target = self.root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text)

    def run(self, *arguments: str, stdin: str = "", cwd: Path | None = None) -> subprocess.CompletedProcess[str]:
        """Run `board` in the board's root, or with `-C` from `cwd`, with only the variables a user's shell passes."""
        uv = shutil.which("uv")
        assert uv is not None, "uv is on PATH"
        environment = {
            "PATH": os.pathsep.join([str(board_binary().parent), str(Path(uv).parent), "/usr/bin", "/bin"]),
            "HOME": os.environ.get("HOME", str(self.root)),
            "XDG_DATA_HOME": str(self.data),
            "XDG_CACHE_HOME": str(self.cache),
            "XDG_CONFIG_HOME": str(self.config),
        }
        flags = [] if cwd is None else ["-C", str(self.root)]
        return subprocess.run(
            [str(board_binary()), *flags, *arguments],
            cwd=cwd or self.root,
            env=environment,
            input=stdin,
            capture_output=True,
            text=True,
            check=False,
        )

    def call(
        self, capability: str, *flags: str, stdin: str = "", cwd: Path | None = None
    ) -> tuple[int, dict[str, Any]]:
        """`board --json call prompts.<capability>` from `cwd`, by default outside any git repository.

        It returns the exit code, and the output or the failure.
        """
        outside = self.root.parent / "outside"
        outside.mkdir(exist_ok=True)
        ran = self.run("--json", "call", f"prompts.{capability}", *flags, stdin=stdin, cwd=cwd or outside)
        document: dict[str, Any] = json.loads(ran.stdout)
        return ran.returncode, document["data"]["output"] if ran.returncode == 0 else document


@pytest.fixture
def board(tmp_path: Path) -> Board:
    """A board in which `board plugin enable prompts` wrote the prompt schema."""
    fixture = Board(
        root=tmp_path / "board", data=tmp_path / "data", cache=tmp_path / "cache", config=tmp_path / "config"
    )
    fixture.root.mkdir()
    subprocess.run(["git", "init", "-q"], cwd=fixture.root, check=True)
    fixture.write(".board/board.json", json.dumps({"$type": "board/config/v1", "commit_on_put": False}))
    plugins = fixture.data / "board" / "plugins"
    plugins.mkdir(parents=True)
    (plugins / "prompts").symlink_to(PLUGIN)
    enabled = fixture.run("plugin", "enable", "prompts")
    assert enabled.returncode == 0, enabled.stderr
    return fixture


def prompt(title: str = "Review a pull request", body: str = "", **fields: Any) -> str:  # noqa: ANN401 - the frontmatter's other fields
    """A `prompts/prompt/v1` Markdown prompt. Each value is written as JSON, which is YAML."""
    values: dict[str, Any] = {
        "$type": "prompts/prompt/v1",
        "title": title,
        "purpose": "A clean-context review of one pull request.",
        "runs_in": {"remote": "example-org/widgets"},
        **fields,
    }
    lines = [f"{key}: {json.dumps(value)}" for key, value in values.items() if value is not None]
    return "---\n" + "\n".join(lines) + "\n---\n" + body


def git(root: Path, *arguments: str) -> str:
    """Run git in `root` and return what it printed, stripped."""
    ran = subprocess.run(
        ["git", "-c", "commit.gpgsign=false", "-c", "user.name=t", "-c", "user.email=t@t", *arguments],
        cwd=root,
        capture_output=True,
        text=True,
        check=True,
    )
    return ran.stdout.strip()


def repo(board: Board, name: str, files: dict[str, str], *, schema: bool = True) -> tuple[Path, str]:
    """A repo beside the board holding `files`, and the `board/mount/v1` pointer that mounts it at its HEAD.

    Without `schema` it lacks the prompt schema, as a notes repo does until `board plugin enable prompts` runs
    in it.
    """
    clone = board.root.parent / name
    git(board.root.parent, "init", "-q", "-b", "main", str(clone))
    every = {
        ".board/board.json": '{"$type": "board/config/v1"}',
        ".board/types/notes/repo/v1.schema.json": NOTES_SCHEMA.read_text(),
        **({".board/types/prompts/prompt/v1.schema.json": SCHEMA.read_text()} if schema else {}),
        **files,
    }
    for path, text in every.items():
        (clone / path).parent.mkdir(parents=True, exist_ok=True)
        (clone / path).write_text(text)
    git(clone, "add", ".")
    git(clone, "commit", "-qm", "fixture")
    pointer = {
        "$type": "board/mount/v1",
        "repo": str(clone),
        "follow": "main",
        "commit": git(clone, "rev-parse", "HEAD"),
    }
    return clone, json.dumps(pointer)


def mark(board: Board, *roots: str) -> None:
    """Add `roots` to the board's `/prompts/repos.json`, the list of the mounts whose prompts it admits."""
    marks = board.root / "prompts" / "repos.json"
    listed: list[str] = json.loads(marks.read_text())["mounts"] if marks.exists() else []
    board.write("prompts/repos.json", json.dumps({"$type": "prompts/repos/v1", "mounts": [*listed, *roots]}))


def mount(board: Board, name: str, files: dict[str, str], *, marked: bool = True, schema: bool = True) -> Path:
    """A repo holding `files`, such as its `notes-repo.json`, mounted at /repos/<name>: its clone.

    With `marked` the board's own tree marks it as a repo whose prompts it admits.
    """
    clone, pointer = repo(board, name, files, schema=schema)
    board.write(f"repos/{name}.json", pointer)
    if marked:
        mark(board, f"/repos/{name}")
    return clone
