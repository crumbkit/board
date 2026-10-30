"""A repository that is its own board, with review enabled, a stub agent in place of a model, and a stub gh.

The stub agent reads the prompt into `prompt.txt`, its environment into
`env.txt`, and prints `answer.txt`, so no test runs a real model. The stub gh
records each call and prints a comment's URL, so no test reaches GitHub. A
personal board takes the plugin's asks, and a `board` shim runs the real
binary, as the owner for a decide when the test approved it.
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
# The variables that keep the owner's and the system's git config out of the tests' own git commands.
ISOLATED = {"GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1"}
REPOSITORY = PLUGIN.parents[1]

AGENT = """#!/bin/sh
cat > "$(dirname "$0")/prompt.txt"
env > "$(dirname "$0")/env.txt"
{ pwd; ls -A; } > "$(dirname "$0")/directory.txt"
cat "$(dirname "$0")/answer.txt"
exit "$(cat "$(dirname "$0")/exit.txt" 2>/dev/null || echo 0)"
"""


# The `board` the plugin runs. A plugin's decide cannot be approved without the owner's signature, so with
# `approve` present the shim runs decide as the owner, whom Board admits, and Board prints its real report.
SHIM = """#!/bin/sh
if [ "$2" = decide ] && [ -e "$(dirname "$0")/approve" ]; then unset BOARD_PLUGIN; fi
exec "{board}" "$@"
"""

GH = """#!/bin/sh
here="$(dirname "$0")"
printf '%s\\n' "$@" > "$here/gh-argv.txt"
cat > "$here/gh-stdin.txt"
echo call >> "$here/gh-calls.txt"
code="$(cat "$here/gh-exit.txt" 2>/dev/null || echo 0)"
if [ "$code" != 0 ]; then echo "HTTP 502: Bad Gateway" >&2; else cat "$here/gh-url.txt" 2>/dev/null || echo "{url}"; fi
exit "$code"
"""
# The comment URL the stub gh prints, unless `gh-url.txt` holds another.
COMMENT_URL = "https://github.com/o/r/pull/7#issuecomment-1"


def board_binary() -> Path:
    """The `board` under test: `$BOARD_BIN`, or the workspace's debug build."""
    return Path(os.environ.get("BOARD_BIN", REPOSITORY / "target" / "debug" / "board")).resolve()


def answer(verdict: str = "approve", findings: list[dict[str, Any]] | None = None) -> str:
    """An agent's answer: some prose, then the JSON object `review.run` reads."""
    return f"Looks fine.\n\n```json\n{json.dumps({'verdict': verdict, 'findings': findings or []})}\n```\n"


@dataclass(frozen=True)
class Repo:
    """A git repository at `root` that holds a board; the stub agent lives in `agent`."""

    root: Path
    agent: Path
    data: Path
    cache: Path
    config: Path
    personal: Path
    bin: Path

    def write(self, path: str, text: str) -> None:
        """Write a fixture file at `path` under the board's root. Every test writes through here."""
        target = self.root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text)

    def git(self, *arguments: str) -> str:
        """Run git in the repository and return what it printed, stripped."""
        ran = subprocess.run(
            ["git", *arguments],
            cwd=self.root,
            env=os.environ | ISOLATED,
            capture_output=True,
            text=True,
            check=True,
        )
        return ran.stdout.strip()

    def commit(self, path: str, text: str) -> str:
        """Write `text` at `path`, commit it, and return the commit."""
        self.write(path, text)
        self.git("add", ".")
        self.git("commit", "-qm", f"change {path}")
        return self.git("rev-parse", "HEAD")

    def answers(self, stdout: str, exit_code: int = 0) -> None:
        """Make the stub agent print `stdout` and exit with `exit_code`."""
        (self.agent / "answer.txt").write_text(stdout)
        (self.agent / "exit.txt").write_text(str(exit_code))

    def prompt(self) -> str:
        """What the stub agent last read."""
        return (self.agent / "prompt.txt").read_text()

    def run(
        self, *arguments: str, stdin: str = "", extra: dict[str, str] | None = None
    ) -> subprocess.CompletedProcess[str]:
        """Run `board` in the board's root, with only the variables a user's shell would pass it, and `extra`.

        BOARD_REVIEW_AGENT names the stub agent unless `extra` sets it; an empty value in `extra` unsets a variable.
        """
        uv = shutil.which("uv")
        assert uv is not None, "uv is on PATH"
        path = os.pathsep.join([str(self.bin), str(board_binary().parent), str(Path(uv).parent), "/usr/bin", "/bin"])
        environment = {
            "PATH": path,
            "HOME": os.environ.get("HOME", str(self.root)),
            "XDG_DATA_HOME": str(self.data),
            "XDG_CACHE_HOME": str(self.cache),
            "XDG_CONFIG_HOME": str(self.config),
            "BOARD_REVIEW_AGENT": json.dumps(["sh", str(self.agent / "agent.sh")]),
        } | (extra or {})
        environment = {name: value for name, value in environment.items() if value}
        return subprocess.run(
            [str(board_binary()), *arguments],
            cwd=self.root,
            env=environment,
            input=stdin,
            capture_output=True,
            text=True,
            check=False,
        )

    def call(self, capability: str, *flags: str, extra: dict[str, str] | None = None) -> tuple[int, dict[str, Any]]:
        """`board --json call review.<capability> <flags>`: its exit code, and its output or failure."""
        ran = self.run("--json", "call", f"review.{capability}", *flags, extra=extra)
        document: dict[str, Any] = json.loads(ran.stdout)
        return ran.returncode, document["data"]["output"] if ran.returncode == 0 else document

    def get(self, path: str) -> dict[str, Any]:
        """The record at the logical `path`, from the working tree."""
        record: dict[str, Any] = json.loads((self.root / f"{path.removeprefix('/')}.json").read_text())
        return record

    def brief(self, name: str, range_: str, **fields: object) -> str:
        """Commit a brief at /reviews/<name>/brief for `range_` and return its logical path."""
        brief = {"$type": "review/brief/v1", "range": range_, "focus": "Check the sync change."} | fields
        self.commit(f"reviews/{name}/brief.json", json.dumps(brief))
        return f"/reviews/{name}/brief"


def _personal(fixture: Repo) -> None:
    """A personal board the known boards file names, and the `board` shim and stub gh on the front of PATH."""
    (fixture.config / "board").mkdir(parents=True)
    (fixture.config / "board" / "config.json").write_text(json.dumps({"personal": str(fixture.personal)}))
    fixture.personal.mkdir()
    (fixture.personal / ".board").mkdir()
    (fixture.personal / ".board" / "board.json").write_text(json.dumps({"$type": "board/config/v1"}))
    # The identity goes in the repository's own config, so Board's commits to it need no global one.
    for arguments in (
        ["init", "-q", "-b", "main"],
        ["config", "user.name", "t"],
        ["config", "user.email", "t@t"],
        ["add", "."],
        ["commit", "-qm", "personal"],
    ):
        subprocess.run(["git", *arguments], cwd=fixture.personal, env=os.environ | ISOLATED, check=True)
    fixture.bin.mkdir()
    for name, script in (("board", SHIM.format(board=board_binary())), ("gh", GH.format(url=COMMENT_URL))):
        (fixture.bin / name).write_text(script)
        (fixture.bin / name).chmod(0o755)


def library(root: Path, files: dict[str, str]) -> dict[str, str]:
    """Commit `files` to a new repository at `root`, and return a mount record pinned to that commit."""
    root.mkdir()
    for path, text in files.items():
        (root / path).write_text(text)
    git = ["git", "-c", "user.name=t", "-c", "user.email=t@t"]
    for arguments in (["init", "-q", "-b", "main"], ["add", "."], ["commit", "-qm", "library"]):
        subprocess.run([*git, *arguments], cwd=root, env=os.environ | ISOLATED, check=True)
    commit = subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=root, env=os.environ | ISOLATED, check=True, capture_output=True, text=True
    ).stdout.strip()
    return {"$type": "board/mount/v1", "repo": str(root), "follow": "main", "commit": commit}


@pytest.fixture
def repo(tmp_path: Path) -> Repo:
    """A repository on main with one commit and its own board enabling review."""
    fixture = Repo(
        root=tmp_path / "repo",
        agent=tmp_path / "agent",
        data=tmp_path / "data",
        cache=tmp_path / "cache",
        config=tmp_path / "config",
        personal=tmp_path / "personal",
        bin=tmp_path / "bin",
    )
    fixture.root.mkdir()
    fixture.agent.mkdir()
    _personal(fixture)
    (fixture.agent / "agent.sh").write_text(AGENT)
    fixture.answers(answer())
    fixture.git("init", "-q", "-b", "main")
    fixture.git("config", "user.name", "t")
    fixture.git("config", "user.email", "t@t")
    fixture.write(".board/board.json", json.dumps({"$type": "board/config/v1"}))
    fixture.commit("README.md", "A repository.\n")
    plugins = fixture.data / "board" / "plugins"
    plugins.mkdir(parents=True)
    (plugins / "review").symlink_to(PLUGIN)
    enabled = fixture.run("plugin", "enable", "review")
    assert enabled.returncode == 0, enabled.stderr
    return fixture
