"""A temporary personal board with boundary and controls enabled, run through the real `board` binary.

Each test names its own personal board in a temporary `XDG_CONFIG_HOME`, so no
test reads or writes the user's own. A stub agent stands in for a model, and a
`board` shim on the front of the plugin's PATH stands in for the owner's signed
approval of an override, which needs Touch ID.
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
# An expression whose text differs from anything it matches, so a quote of a match never reveals it.
PRIVATE = r"acme-\d{3}"


# The `board` the plugin runs. An approval needs the owner's Touch ID, so for a decide at a path that a line
# of `approved` names, with the effect id after it, the shim runs decide as the owner, whom Board admits, and
# names that effect in the report, as Board's report of a signed approval does. Every other run is the binary's.
SHIM = """#!/bin/sh
here="$(dirname "$0")"
at=""; previous=""; deciding=""
for argument in "$@"; do
  [ "$argument" = decide ] && deciding=1
  [ "$previous" = --path ] && at="$argument"
  previous="$argument"
done
effect="$(awk -v at="$at" '$1 == at {{ print $2 }}' "$here/approved" 2>/dev/null)"
if [ -n "$deciding" ] && [ -n "$effect" ]; then
  unset BOARD_PLUGIN
  "{board}" "$@" | sed "s/\\"effect\\": *null/\\"effect\\":\\"$effect\\"/"
  exit 0
fi
exec "{board}" "$@"
"""


def board_binary() -> Path:
    """The `board` under test: `$BOARD_BIN`, or the workspace's debug build."""
    return Path(os.environ.get("BOARD_BIN", REPOSITORY / "target" / "debug" / "board")).resolve()


@dataclass(frozen=True)
class Board:
    """A git repository holding a board, with plugins installed in a data directory it shares with other boards."""

    root: Path
    home: Path

    def write(self, path: str, text: str) -> None:
        """Write a fixture file at `path`, relative to the board's root."""
        target = self.root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text)

    def run(
        self, *arguments: str, stdin: str = "", extra: dict[str, str] | None = None
    ) -> subprocess.CompletedProcess[str]:
        """Run `board` in the board's root, with only the variables a user's shell would pass it, and `extra`."""
        uv = shutil.which("uv")
        assert uv is not None, "uv is on PATH"
        path = os.pathsep.join(
            str(directory)
            for directory in [self.home / "bin", board_binary().parent, Path(uv).parent, "/usr/bin", "/bin"]
        )
        environment = {
            "PATH": path,
            "HOME": os.environ.get("HOME", str(self.home)),
            "XDG_DATA_HOME": str(self.home / "data"),
            "XDG_CACHE_HOME": str(self.home / "cache"),
            "XDG_CONFIG_HOME": str(self.home / "config"),
            "GIT_CONFIG_GLOBAL": os.devnull,
            "GIT_CONFIG_NOSYSTEM": "1",
        } | (extra or {})
        return subprocess.run(
            [str(board_binary()), *arguments],
            cwd=self.root,
            env=environment,
            input=stdin,
            capture_output=True,
            text=True,
            check=False,
        )

    def call(
        self, capability: str, given: dict[str, Any], extra: dict[str, str] | None = None
    ) -> tuple[subprocess.CompletedProcess[str], dict[str, Any]]:
        """Run `board --json call boundary.<capability>` with the input on stdin, and read its one document."""
        ran = self.run("--json", "call", f"boundary.{capability}", stdin=json.dumps(given), extra=extra)
        document: dict[str, Any] = json.loads(ran.stdout)
        return ran, document

    def approve(self, path: str, effect: str) -> None:
        """Make the `board` shim admit the plugin's decide at `path` as the owner's approval of `effect` would."""
        with (self.home / "bin" / "approved").open("a") as approved:
            approved.write(f"{path} {effect}\n")

    def record(self, path: str) -> dict[str, Any] | None:
        """The record at `path`, read from the working tree, or None."""
        file = self.root / f"{path.lstrip('/')}.json"
        return json.loads(file.read_text()) if file.exists() else None


def make_board(root: Path, home: Path, plugins: list[str]) -> Board:
    """A new board at `root` that does not commit, with `plugins` enabled."""
    board = Board(root=root, home=home)
    root.mkdir(parents=True)
    subprocess.run(["git", "init", "-q"], cwd=root, check=True)
    board.write(".board/board.json", json.dumps({"$type": "board/config/v1", "commit_on_put": False}))
    for plugin in plugins:
        enabled = board.run("plugin", "enable", plugin)
        assert enabled.returncode == 0, enabled.stderr
    return board


@pytest.fixture
def personal(tmp_path: Path) -> Board:
    """The personal board, with boundary and controls installed and enabled, named in the known boards file."""
    home = tmp_path / "home"
    installed = home / "data" / "board" / "plugins"
    installed.mkdir(parents=True)
    (installed / "boundary").symlink_to(PLUGIN)
    (installed / "controls").symlink_to(REPOSITORY / "plugins" / "controls")
    config = home / "config" / "board"
    config.mkdir(parents=True)
    (config / "config.json").write_text(json.dumps({"personal": str(tmp_path / "personal")}))
    shim = home / "bin" / "board"
    shim.parent.mkdir()
    shim.write_text(SHIM.format(board=board_binary()))
    shim.chmod(0o755)
    return make_board(tmp_path / "personal", home, ["controls", "boundary"])


def control(path: str, rule: dict[str, Any]) -> tuple[str, str]:
    """A `controls/control/v1` record with `rule`, as a fixture file and its text."""
    record = {"$type": "controls/control/v1", "requirement": "A test.", "purpose": "A test.", "rule": rule}
    return f"{path.lstrip('/')}.json", json.dumps(record)


def boundary(**fields: Any) -> str:  # noqa: ANN401 - the record's fields, as JSON
    """A `boundary/boundary/v1` record."""
    return json.dumps({"$type": "boundary/boundary/v1", **fields})


def private_names(board: Board) -> None:
    """A pattern control at /controls/names, shown match-only, whose file holds PRIVATE."""
    board.write("private/names.txt", f"{PRIVATE}\n")
    board.write(*control("/controls/names", {"pattern": "/private/names.txt", "show": "match-only"}))


# A stub in place of a model: it counts its calls and prints `answer.txt` beside it.
AGENT = """#!/bin/sh
here="$(dirname "$0")"
cat > /dev/null
echo call >> "$here/calls.txt"
cat "$here/answer.txt"
"""


@dataclass(frozen=True)
class Agent:
    """The stub agent in `directory`, which `controls.evaluate` starts through BOARD_CONTROLS_AGENT."""

    directory: Path

    @property
    def variables(self) -> dict[str, str]:
        """The variable that names the agent."""
        return {"BOARD_CONTROLS_AGENT": json.dumps([str(self.directory / "agent")])}

    def answers(self, findings: list[dict[str, Any]]) -> None:
        """Make the agent answer with some prose and then `findings`."""
        (self.directory / "answer.txt").write_text(f"Read it.\n{json.dumps({'findings': findings})}\n")

    def calls(self) -> int:
        """How many times the agent ran."""
        calls = self.directory / "calls.txt"
        return len(calls.read_text().splitlines()) if calls.exists() else 0


@pytest.fixture
def agent(tmp_path: Path) -> Agent:
    """A stub agent that answers with no findings until told otherwise."""
    stub = Agent(tmp_path / "agent")
    stub.directory.mkdir()
    script = stub.directory / "agent"
    script.write_text(AGENT)
    script.chmod(0o755)
    stub.answers([])
    return stub
