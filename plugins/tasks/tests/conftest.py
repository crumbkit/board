"""Fixture boards that run the real `board` binary with this plugin installed."""

import datetime as dt
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
SCHEMA_V2 = PLUGIN / "types" / "tasks" / "task" / "v2.schema.json"
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

    def enable(self, *plugins: str) -> None:
        """Make `.board/board.json` enable `plugins`, without committing a put."""
        config = {"$type": "board/config/v1", "commit_on_put": False, "plugins": list(plugins)}
        self.write(".board/board.json", json.dumps(config))

    def run(
        self, *arguments: str, stdin: str = "", cwd: Path | None = None, tz: str | None = None
    ) -> subprocess.CompletedProcess[str]:
        """Run `board` in the board's root, or with `-C` from `cwd`, with only the variables a user's shell passes."""
        uv = shutil.which("uv")
        assert uv is not None, "uv is on PATH"
        path = os.pathsep.join([str(board_binary().parent), str(Path(uv).parent), "/usr/bin", "/bin"])
        environment = {
            "PATH": path,
            "HOME": os.environ.get("HOME", str(self.root)),
            "XDG_DATA_HOME": str(self.data),
            "XDG_CACHE_HOME": str(self.cache),
            "XDG_CONFIG_HOME": str(self.config),
        } | ({"TZ": tz} if tz else {})
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

    def json(self, *arguments: str, stdin: str = "") -> dict[str, Any]:
        """Run `board --json` and read its one document."""
        ran = self.run("--json", *arguments, stdin=stdin)
        document: dict[str, Any] = json.loads(ran.stdout)
        return document


@pytest.fixture
def board(tmp_path: Path) -> Board:
    """A board enabling digest and tasks, which `board plugin enable` gave the task schema."""
    fixture = Board(
        root=tmp_path / "board", data=tmp_path / "data", cache=tmp_path / "cache", config=tmp_path / "config"
    )
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


def task_v2(title: str, status: str = "open", body: str = "", **fields: Any) -> str:  # noqa: ANN401 - the frontmatter's other fields
    """A `tasks/task/v2` Markdown task. Each value is written as JSON, which is YAML."""
    values: dict[str, Any] = {"$type": "tasks/task/v2", "title": title, "status": status, **fields}
    lines = [f"{key}: {json.dumps(value)}" for key, value in values.items()]
    return "---\n" + "\n".join(lines) + "\n---\n" + body


# The furthest ahead of UTC a time zone runs, in hours.
LATEST_OFFSET = 14


def zone_at_one_in_the_morning() -> str:
    """A fixed-offset time zone whose local time is now between 01:00 and 01:59."""
    offset = (1 - dt.datetime.now(dt.UTC).hour) % 24
    offset = offset - 24 if offset > LATEST_OFFSET else offset
    # Etc/GMT's sign is POSIX's, the opposite of ISO's: Etc/GMT-3 is three hours ahead of UTC.
    return "Etc/GMT" if offset == 0 else f"Etc/GMT{-offset:+d}"


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


def notes_repo(
    board: Board, name: str, record: dict[str, Any] | None, tasks: dict[str, str], *, daily: bool = True
) -> Path:
    """A notes repo pushed to a bare remote, holding `tasks` and `record`, mounted at /repos/<name>: its clone.

    Without `daily` it has no daily/, so it is a mount that is not a notes repo.
    """
    parent = board.root.parent
    remote, clone = parent / f"{name}.git", parent / name
    git(parent, "init", "-q", "--bare", "-b", "main", str(remote))
    git(parent, "init", "-q", "-b", "main", str(clone))
    git(clone, "config", "user.name", "t")
    git(clone, "config", "user.email", "t@t")
    files = {
        ".board/board.json": '{"$type": "board/config/v1"}',
        ".board/types/tasks/task/v2.schema.json": SCHEMA_V2.read_text(),
        ".board/types/notes/repo/v1.schema.json": NOTES_SCHEMA.read_text(),
        **({"daily/.gitkeep": ""} if daily else {}),
        "repo.txt": name,
        **({"notes-repo.json": json.dumps({"$type": "notes/repo/v1", **record})} if record else {}),
        **{f"tasks/{slug}.md": text for slug, text in tasks.items()},
    }
    for path, text in files.items():
        (clone / path).parent.mkdir(parents=True, exist_ok=True)
        (clone / path).write_text(text)
    git(clone, "add", ".")
    git(clone, "commit", "-qm", "fixture")
    git(clone, "remote", "add", "origin", str(remote))
    git(clone, "push", "-qu", "origin", "main")
    pointer = {
        "$type": "board/mount/v1",
        "repo": str(clone),
        "follow": "main",
        "commit": git(clone, "rev-parse", "HEAD"),
    }
    board.write(f"repos/{name}.json", json.dumps(pointer))
    return clone
