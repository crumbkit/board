"""A plugin migrating its records to the types it now ships, against a fake `board` and through the real one."""

import json
import os
import subprocess
import sys
import textwrap
from pathlib import Path
from typing import Any

import pytest

from board_plugin import migrate
from board_plugin.board import Board
from board_plugin.migrate import Old

REPOSITORY = Path(__file__).resolve().parents[3]
DEMO = Board("/b", "demo")
OLD = Old("/items/one", "a" * 40, {"$type": "demo/a/v1", "name": "one"})


def test_lines_expect_the_blob_read_and_a_move_writes_a_new_file_then_removes_the_old() -> None:
    assert migrate.put(OLD, {"$type": "demo/b/v1"}) == {
        "path": "/items/one",
        "expect": "a" * 40,
        "record": {"$type": "demo/b/v1"},
    }
    assert migrate.move(OLD, "/items/one.md", "text") == [
        {"path": "/items/one.md", "expect": "none", "text": "text"},
        {"path": "/items/one", "expect": "a" * 40, "remove": True},
    ]


def recording(monkeypatch: pytest.MonkeyPatch) -> list[tuple[tuple[str, ...], bytes]]:
    """Make every `board` the library runs answer a put that made commit `c`, and record each run."""
    runs: list[tuple[tuple[str, ...], bytes]] = []

    def run(_self: Board, *arguments: str, stdin: bytes = b"", **_: object) -> subprocess.CompletedProcess[bytes]:
        runs.append((arguments, stdin))
        report = json.dumps({"schema": "board/put-report/v2", "data": {"commit": "c"}})
        return subprocess.CompletedProcess(["board", *arguments], 0, report.encode(), b"")

    monkeypatch.setattr(Board, "_run", run)
    return runs


def test_a_dry_run_starts_no_board_and_an_applied_one_starts_one_batch_of_every_line(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    runs = recording(monkeypatch)
    lines = [migrate.put(OLD, {"$type": "demo/b/v1"}), migrate.remove(OLD)]
    dry = migrate.migrate(DEMO, lines, ["re-sign /items/one"], apply=False)
    assert runs == []
    assert dry == {
        "applied": False,
        "commit": None,
        "lines": lines,
        "total": 2,
        "truncated": False,
        "owed": ["re-sign /items/one"],
    }
    applied = migrate.migrate(DEMO, lines, apply=True)
    assert [arguments for arguments, _ in runs] == [("--json", "put", "--batch")]
    assert [json.loads(line) for line in runs[0][1].decode().splitlines()] == lines
    assert applied["commit"] == "c"


def test_the_output_lists_at_most_its_limit_of_lines_and_counts_them_all(monkeypatch: pytest.MonkeyPatch) -> None:
    recording(monkeypatch)
    lines = [migrate.remove(OLD)] * (migrate.LIST_LIMIT + 1)
    listed = migrate.migrate(DEMO, lines, apply=False)
    assert len(listed["lines"]) == migrate.LIST_LIMIT  # type: ignore[arg-type]
    assert (listed["total"], listed["truncated"]) == (migrate.LIST_LIMIT + 1, True)
    fits = migrate.migrate(DEMO, lines[1:], apply=False)
    assert (fits["total"], fits["truncated"]) == (migrate.LIST_LIMIT, False)


# The fixture plugin's program: `migrate` moves each `demo/a/v1` record to `demo/b/v1`.
PROGRAM = """
import sys

from board_plugin.board import Board
from board_plugin.migrate import migrate, move, olds, put
from board_plugin.protocol import main


def call(request):
    board = Board(request.board.root, "demo")
    lines = []
    for old in olds(board, ["demo/a/v1"]):
        if old.path.endswith(".md"):
            text = f"---\\n$type: demo/b/v1\\nname: {old.record['name']}\\n---\\n"
            lines += move(old, old.path.removesuffix(".md") + "-b.md", text)
        else:
            lines.append(put(old, {"$type": "demo/b/v1", "name": old.record["name"]}))
    owed = [f"re-sign {old.path}" for old in olds(board, ["demo/a/v1"])]
    return migrate(board, lines, owed, apply=request.input.get("apply") is True)


sys.exit(main("demo", call))
"""

SCHEMA = '{"type": "object", "required": ["name"]}\n'


def install(data: Path, ships: str, *, capabilities: bool) -> None:
    """Install the demo plugin under `data`, shipping the type `ships`, with `migrate` when `capabilities`."""
    directory = data / "board" / "plugins" / "demo"
    for old in directory.rglob("*.schema.json"):
        old.unlink()
    schema = directory / "types" / f"{ships}.schema.json"
    schema.parent.mkdir(parents=True, exist_ok=True)
    schema.write_text(SCHEMA)
    (directory / "main.py").write_text(textwrap.dedent(PROGRAM))
    offered = {
        "name": "migrate",
        "description": "Move demo/a/v1 records to demo/b/v1.",
        "inputSchema": {"type": "object", "properties": {"apply": {"type": "boolean"}}},
        "outputSchema": {"type": "object"},
    }
    manifest = {
        "$type": "board/plugin-manifest/v1",
        "name": "demo",
        "version": "2.0.0" if capabilities else "1.0.0",
        "description": "Demo.",
        "run": [sys.executable, "main.py"],
        "types": [ships],
        "capabilities": [offered] if capabilities else [],
    }
    (directory / "plugin.json").write_text(json.dumps(manifest))


class Real:
    """A board, and a plugins directory holding the demo plugin, run through the workspace's debug `board`."""

    def __init__(self, tmp_path: Path) -> None:
        """Make the board and install the first version of the plugin."""
        self.root = tmp_path / "board"
        self.data = tmp_path / "data"
        self.environment = {
            "PATH": os.pathsep.join([str(binary().parent), "/usr/bin", "/bin"]),
            "HOME": str(tmp_path),
            "XDG_DATA_HOME": str(self.data),
            "XDG_CACHE_HOME": str(tmp_path / "cache"),
            "XDG_CONFIG_HOME": str(tmp_path / "config"),
        }
        self.root.mkdir()
        self.git("init", "-q")
        self.git("config", "user.name", "t")
        self.git("config", "user.email", "t@t")
        install(self.data, "demo/a/v1", capabilities=False)
        assert self.board("init").returncode == 0

    def git(self, *arguments: str) -> str:
        """What git printed in the board, stripped."""
        ran = subprocess.run(["git", *arguments], cwd=self.root, capture_output=True, text=True, check=True)
        return ran.stdout.strip()

    def board(self, *arguments: str, stdin: str = "") -> subprocess.CompletedProcess[str]:
        """Run `board` in the board, with only the variables a user's shell would pass it."""
        return subprocess.run(
            [str(binary()), *arguments],
            cwd=self.root,
            env=self.environment,
            input=stdin,
            capture_output=True,
            text=True,
            check=False,
        )

    def document(self, *arguments: str, stdin: str = "") -> dict[str, Any]:
        """The document `board --json` prints for a run that succeeds."""
        ran = self.board("--json", *arguments, stdin=stdin)
        assert ran.returncode == 0, ran.stdout + ran.stderr
        document: dict[str, Any] = json.loads(ran.stdout)
        return document


def binary() -> Path:
    """The `board` under test: `$BOARD_BIN`, or the workspace's debug build."""
    return Path(os.environ.get("BOARD_BIN", REPOSITORY / "target" / "debug" / "board")).resolve()


def test_a_plugin_that_dropped_a_type_still_runs_and_migrates_its_records_in_one_commit(tmp_path: Path) -> None:
    real = Real(tmp_path)
    real.document("plugin", "enable", "demo")
    batch = [
        {"path": "/items/one", "record": {"$type": "demo/a/v1", "name": "one"}},
        {"path": "/notes/two.md", "text": "---\n$type: demo/a/v1\nname: two\n---\n"},
    ]
    real.document("put", "--batch", stdin="".join(json.dumps(line) + "\n" for line in batch))

    install(real.data, "demo/b/v1", capabilities=True)
    enabled = real.document("plugin", "enable", "demo")["data"]
    old = ".board/types/demo/a/v1.schema.json"
    assert enabled["kept"] == [{"schema": old, "type": "demo/a/v1", "records": 2}]
    assert enabled["removed"] == []

    head = real.git("rev-parse", "HEAD")
    dry = real.document("call", "demo.migrate", stdin="{}")["data"]["output"]
    assert real.git("rev-parse", "HEAD") == head, "a dry run writes nothing"
    assert real.git("status", "--porcelain") == ""
    assert dry["applied"] is False
    assert [line["path"] for line in dry["lines"]] == ["/items/one", "/notes/two-b.md", "/notes/two.md"]
    assert dry["owed"] == ["re-sign /items/one", "re-sign /notes/two.md"]

    applied = real.document("call", "demo.migrate", stdin='{"apply": true}')["data"]["output"]
    assert real.git("rev-parse", "HEAD~1") == head, "one commit"
    assert applied["commit"] == real.git("rev-parse", "HEAD")
    assert real.git("show", "HEAD:items/one.json").count("demo/b/v1") == 1
    assert real.git("ls-tree", "-r", "--name-only", "HEAD", "notes") == "notes/two-b.md"

    enabled = real.document("plugin", "enable", "demo")["data"]
    assert enabled["kept"] == []
    assert enabled["removed"] == [old]
    assert not (real.root / old).exists()
