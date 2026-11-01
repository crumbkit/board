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
    """Run git in `root` with an identity of its own and no global config, and return its output."""
    identity = ["-c", "user.name=t", "-c", "user.email=t@t"]
    alone = os.environ | {"GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1"}
    ran = subprocess.run(
        ["git", *identity, *arguments], cwd=root, env=alone, capture_output=True, text=True, check=True
    )
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


# The stub gh for the sending actions: as GH, and it also keeps what it read on stdin in `gh-stdin.txt`.
GH_SENDING = GH.replace(
    'echo call >> "$here/gh-calls.txt"', 'echo call >> "$here/gh-calls.txt"\ncat > "$here/gh-stdin.txt"'
)
# The remote a checkout names. Its own config sends git to a local bare repository in its place.
ORIGIN = "https://github.com/Octo/Widgets.git"
# A pattern a test's control refuses, and the control's path in the personal board.
PRIVATE = r"acme-\d{3}"
NAMES = "/controls/names"


@dataclass(frozen=True)
class Desk:
    """A personal board with github, boundary and controls enabled, a checkout it mounts, and the checkout's remote.

    The checkout's origin names GitHub, and its git config sends every fetch
    and push to the bare repository at `remote` instead, so no test reaches a
    network. HOME is a temporary directory, so no global git config applies.
    """

    personal: Path
    checkout: Path
    remote: Path
    home: Path
    bin: Path

    def environment(self) -> dict[str, str]:
        """The variables a shell would pass `board`, with the stub gh first on PATH."""
        uv = shutil.which("uv")
        assert uv is not None, "uv is on PATH"
        path = os.pathsep.join([str(self.bin), str(board_binary().parent), str(Path(uv).parent), "/usr/bin", "/bin"])
        return {
            "PATH": path,
            "HOME": str(self.home),
            "XDG_DATA_HOME": str(self.home / "data"),
            "XDG_CACHE_HOME": str(self.home / "cache"),
            "XDG_CONFIG_HOME": str(self.home / "config"),
            "GIT_CONFIG_NOSYSTEM": "1",
        }

    def run(
        self, *arguments: str, cwd: Path | None = None, stdin: str = "", extra: dict[str, str] | None = None
    ) -> subprocess.CompletedProcess[str]:
        """Run `board` in the checkout, or in `cwd`, with the `extra` variables."""
        return subprocess.run(
            [str(board_binary()), *arguments],
            cwd=cwd or self.checkout,
            env=self.environment() | (extra or {}),
            input=stdin,
            capture_output=True,
            text=True,
            check=False,
        )

    def call(
        self,
        capability: str,
        *flags: str,
        cwd: Path | None = None,
        personal: bool = False,
        extra: dict[str, str] | None = None,
    ) -> dict[str, Any]:
        """The one document `board --json call github.<capability> <flags>` prints, with its exit code as `exit`."""
        front = ["--personal"] if personal else []
        ran = self.run(*front, "--json", "call", f"github.{capability}", *flags, cwd=cwd, extra=extra)
        document: dict[str, Any] = json.loads(ran.stdout)
        return document | {"exit": ran.returncode}

    def write(self, path: str, text: str) -> None:
        """Write a fixture file at `path` in the personal board. Every test writes there through here."""
        target = self.personal / path.lstrip("/")
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text)

    def record(self, path: str) -> dict[str, Any] | None:
        """The record at the logical `path` in the personal board, or None."""
        file = self.personal / f"{path.lstrip('/')}.json"
        return json.loads(file.read_text()) if file.exists() else None

    def refuse(self, parts: list[str], actions: str = "github.*") -> None:
        """A boundary on octo's repositories that checks `parts` of `actions` against a control refusing PRIVATE."""
        self.write("private/names.txt", f"{PRIVATE}\n")
        control = {"pattern": "/private/names.txt", "show": "match-only"}
        self.write(
            f"{NAMES}.json",
            json.dumps(
                {"$type": "controls/control/v1", "requirement": "A test.", "purpose": "A test.", "rule": control}
            ),
        )
        checks = [{"control": NAMES, "parts": parts}]
        record = {"$type": "boundary/boundary/v1", "to": "github:octo/*", "actions": [actions], "checks": checks}
        self.write("boundaries/octo.json", json.dumps(record))

    def commit(self, message: str, file: str = "widget.txt", text: str | None = None) -> str:
        """Commit a change to `file` in the checkout and return the commit's full SHA."""
        (self.checkout / file).write_text(text if text is not None else message + "\n")
        git(self.checkout, "add", file)
        git(self.checkout, "commit", "-q", "-m", message)
        return git(self.checkout, "rev-parse", "HEAD")

    def remote_head(self, branch: str) -> str | None:
        """The commit the bare remote's `branch` points at, or None."""
        ran = subprocess.run(
            ["git", "rev-parse", "--verify", "-q", f"refs/heads/{branch}"],
            cwd=self.remote,
            capture_output=True,
            text=True,
            check=False,
        )
        return ran.stdout.strip() or None

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

    def gh_stdin(self) -> str:
        """What the stub gh read on stdin in its last run."""
        return (self.bin / "gh-stdin.txt").read_text()


@pytest.fixture
def desk(tmp_path: Path) -> Desk:
    """A checkout on `main`, pushed to its remote, mounted at /repos/widgets in a personal board."""
    tmp_path = tmp_path.resolve()
    fixture = Desk(
        personal=tmp_path / "personal",
        checkout=tmp_path / "widgets",
        remote=tmp_path / "remote.git",
        home=tmp_path / "home",
        bin=tmp_path / "bin",
    )
    plugins = fixture.home / "data" / "board" / "plugins"
    plugins.mkdir(parents=True)
    for name in ("github", "boundary", "controls"):
        (plugins / name).symlink_to(REPOSITORY / "plugins" / name)
    config = fixture.home / "config" / "board"
    config.mkdir(parents=True)
    known = {"boards": [str(fixture.personal)], "personal": str(fixture.personal)}
    (config / "config.json").write_text(json.dumps(known))
    fixture.bin.mkdir()
    (fixture.bin / "gh").write_text(GH_SENDING)
    (fixture.bin / "gh").chmod(0o755)
    subprocess.run(["git", "init", "-q", "--bare", "-b", "main", str(fixture.remote)], check=True)
    fixture.checkout.mkdir()
    git(fixture.checkout, "init", "-q", "-b", "main")
    git(fixture.checkout, "remote", "add", "origin", ORIGIN)
    git(fixture.checkout, "config", f"url.{fixture.remote}.insteadOf", ORIGIN)
    git(fixture.checkout, "config", "user.name", "t")
    git(fixture.checkout, "config", "user.email", "t@t")
    head = fixture.commit("Start")
    git(fixture.checkout, "push", "-q", "-u", "origin", "main")
    fixture.personal.mkdir()
    git(fixture.personal, "init", "-q")
    fixture.write(".board/board.json", json.dumps({"$type": "board/config/v1", "commit_on_put": False}))
    mount = {"$type": "board/mount/v1", "repo": str(fixture.checkout), "follow": "origin/main", "commit": head}
    fixture.write("repos/widgets.json", json.dumps(mount))
    for name in ("controls", "boundary", "github"):
        enabled = fixture.run("plugin", "enable", name, cwd=fixture.personal)
        assert enabled.returncode == 0, enabled.stderr
    return fixture
