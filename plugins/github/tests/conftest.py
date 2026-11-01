"""A board with this plugin enabled, and a stub gh on the front of PATH so no test reaches GitHub.

The stub records its arguments in `gh-argv.txt` and each run in
`gh-calls.txt`, prints `gh-out.txt` to stdout and `gh-err.txt` to stderr,
and exits with the code in `gh-exit.txt`, 0 when that file is absent.
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

GH = """#!/bin/sh
here="$(dirname "$0")"
printf '%s\\n' "$@" > "$here/gh-argv.txt"
echo call >> "$here/gh-calls.txt"
cat "$here/gh-out.txt" 2>/dev/null
cat "$here/gh-err.txt" >&2 2>/dev/null
exit "$(cat "$here/gh-exit.txt" 2>/dev/null || echo 0)"
"""


def board_binary() -> Path:
    """The `board` under test: `$BOARD_BIN`, or the workspace's debug build."""
    return Path(os.environ.get("BOARD_BIN", REPOSITORY / "target" / "debug" / "board")).resolve()


def git(root: Path, *arguments: str) -> str:
    """Run git in `root` with an identity of its own and return its output."""
    identity = ["-c", "user.name=t", "-c", "user.email=t@t"]
    ran = subprocess.run(["git", *identity, *arguments], cwd=root, capture_output=True, text=True, check=True)
    return ran.stdout.strip()


def issue(number: int, title: str = "Widgets wobble", *, pull: bool = False) -> dict[str, Any]:
    """What `gh api repos/octo/widgets/issues/<number>` prints, cut to the fields the plugin reads and one more."""
    kind = "pull" if pull else "issues"
    answer: dict[str, Any] = {
        "number": number,
        "title": title,
        "html_url": f"https://github.com/octo/widgets/{kind}/{number}",
        "state": "open",
    }
    if pull:
        answer["pull_request"] = {"url": f"https://api.github.com/repos/octo/widgets/pulls/{number}"}
    return answer


@dataclass(frozen=True)
class Board:
    """A git repository holding a board, with this plugin installed for it alone, and the stub gh."""

    root: Path
    data: Path
    cache: Path
    bin: Path

    def write(self, path: str, text: str) -> None:
        """Write a fixture file at `path`, relative to the board's root. Every test writes through here."""
        target = self.root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text)

    def read(self, path: str) -> dict[str, Any]:
        """The JSON record at `path`, relative to the board's root."""
        record: dict[str, Any] = json.loads((self.root / path).read_text())
        return record

    def gh_answers(self, answer: dict[str, Any] | str, code: int = 0, stderr: str = "") -> None:
        """Make the stub gh print `answer`, as JSON unless it is text, and `stderr`, and exit with `code`."""
        (self.bin / "gh-out.txt").write_text(answer if isinstance(answer, str) else json.dumps(answer))
        (self.bin / "gh-err.txt").write_text(stderr)
        (self.bin / "gh-exit.txt").write_text(str(code))

    def gh_calls(self) -> int:
        """How many times the stub gh ran."""
        calls = self.bin / "gh-calls.txt"
        return len(calls.read_text().splitlines()) if calls.exists() else 0

    def gh_argv(self) -> list[str]:
        """The arguments of the stub gh's last run."""
        return (self.bin / "gh-argv.txt").read_text().splitlines()

    def run(self, *arguments: str, gh: bool = True) -> subprocess.CompletedProcess[str]:
        """Run `board` in the board's root with the variables a shell would pass it; `gh` puts gh on PATH."""
        uv = shutil.which("uv")
        assert uv is not None, "uv is on PATH"
        front = [str(self.bin)] if gh else []
        path = os.pathsep.join([*front, str(board_binary().parent), str(Path(uv).parent), "/usr/bin", "/bin"])
        environment = {
            "PATH": path,
            "HOME": os.environ.get("HOME", str(self.root)),
            "XDG_DATA_HOME": str(self.data),
            "XDG_CACHE_HOME": str(self.cache),
        }
        return subprocess.run(
            [str(board_binary()), *arguments],
            cwd=self.root,
            env=environment,
            capture_output=True,
            text=True,
            check=False,
        )

    def json(self, *arguments: str) -> dict[str, Any]:
        """Run `board --json` and read its one document."""
        document: dict[str, Any] = json.loads(self.run("--json", *arguments).stdout)
        return document

    def call(self, ref: str) -> dict[str, Any]:
        """The output of `board call github.ref --ref <ref>`."""
        report = self.json("call", "github.ref", "--ref", ref)
        assert report.get("schema") == "board/call-report/v2", report
        output: dict[str, Any] = report["data"]["output"]
        return output

    def refusal(self, ref: str, *, gh: bool = True) -> dict[str, Any]:
        """The failure `board call github.ref --ref <ref>` prints when it is refused."""
        ran = self.run("--json", "call", "github.ref", "--ref", ref, gh=gh)
        assert ran.returncode != 0, ran.stdout
        failure: dict[str, Any] = json.loads(ran.stdout)
        return failure


@pytest.fixture
def board(tmp_path: Path) -> Board:
    """A board that does not commit, at a root holding a space, with github enabled and the stub gh."""
    fixture = Board(root=tmp_path / "a board", data=tmp_path / "data", cache=tmp_path / "cache", bin=tmp_path / "bin")
    fixture.root.mkdir()
    git(fixture.root, "init", "-q")
    fixture.write(".board/board.json", json.dumps({"$type": "board/config/v1", "commit_on_put": False}))
    plugins = fixture.data / "board" / "plugins"
    plugins.mkdir(parents=True)
    (plugins / "github").symlink_to(PLUGIN)
    fixture.bin.mkdir()
    (fixture.bin / "gh").write_text(GH)
    (fixture.bin / "gh").chmod(0o755)
    enabled = fixture.run("plugin", "enable", "github")
    assert enabled.returncode == 0, enabled.stderr
    return fixture
