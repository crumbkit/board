"""A board that mounts a vault at /vaults/personal, the vault's remote, and a phone that pushes to it.

Each is a real git repository. The vault is the clone Reflect's desktop app
would hold, and the phone another clone of the same remote.
"""

import datetime as dt
import json
import os
import shutil
import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import Any
from zoneinfo import ZoneInfo

import pytest

PLUGIN = Path(__file__).resolve().parents[1]
REPOSITORY = PLUGIN.parents[1]


def board_binary() -> Path:
    """The `board` under test: `$BOARD_BIN`, or the workspace's debug build."""
    return Path(os.environ.get("BOARD_BIN", REPOSITORY / "target" / "debug" / "board")).resolve()


def git(root: Path, *arguments: str) -> str:
    """Run git in `root` and return what it printed, stripped."""
    ran = subprocess.run(
        ["git", "-c", "commit.gpgsign=false", *arguments], cwd=root, capture_output=True, text=True, check=True
    )
    return ran.stdout.strip()


def dates(zone: str) -> tuple[str, str]:
    """Today's date in the time zone `zone`, read now and an hour on, so a call between them is on one of the two."""
    now = dt.datetime.now(ZoneInfo(zone))
    return now.date().isoformat(), (now + dt.timedelta(hours=1)).date().isoformat()


@dataclass(frozen=True)
class Vaults:
    """`board`, which mounts `vault` at /vaults/personal; `remote`, which `vault` and `phone` clone."""

    board: Path
    vault: Path
    remote: Path
    phone: Path
    data: Path
    cache: Path

    def write(self, root: Path, path: str, text: str) -> None:
        """Write a fixture file at `path` under `root`. Every test writes through here."""
        target = root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text)

    def run(self, *arguments: str, tz: str | None = None) -> subprocess.CompletedProcess[str]:
        """Run `board` in the board, with only the variables a user's shell would pass it, and `TZ` when given."""
        uv = shutil.which("uv")
        assert uv is not None, "uv is on PATH"
        path = os.pathsep.join([str(board_binary().parent), str(Path(uv).parent), "/usr/bin", "/bin"])
        environment = {
            "PATH": path,
            "HOME": os.environ.get("HOME", str(self.board)),
            "XDG_DATA_HOME": str(self.data),
            "XDG_CACHE_HOME": str(self.cache),
        } | ({"TZ": tz} if tz else {})
        return subprocess.run(
            [str(board_binary()), *arguments],
            cwd=self.board,
            env=environment,
            capture_output=True,
            text=True,
            check=False,
        )

    def call(self, capability: str, *flags: str, tz: str | None = None) -> tuple[int, dict[str, Any]]:
        """`board --json call notes.<capability> <flags>`: its exit code, and its output or failure."""
        ran = self.run("--json", "call", f"notes.{capability}", *flags, tz=tz)
        document: dict[str, Any] = json.loads(ran.stdout)
        return ran.returncode, document["data"]["output"] if ran.returncode == 0 else document

    def phone_appends(self, path: str, text: str) -> None:
        """Pull on the phone, append `text` to the file at `path`, commit and push, as Reflect's phone app does."""
        git(self.phone, "pull", "-q")
        target = self.phone / path
        self.write(self.phone, path, (target.read_text() if target.exists() else "") + text)
        commit(self.phone)
        git(self.phone, "push", "-q")

    def note(self, output: dict[str, Any]) -> str:
        """The path in the vault of the note a call's `output` names."""
        return str(output["path"]).removeprefix("/vaults/personal/")

    def pin(self) -> str:
        """The commit the board's mount of the vault reads."""
        pointer: dict[str, str] = json.loads((self.board / "vaults" / "personal.json").read_text())
        return pointer["commit"]


def repository(root: Path) -> None:
    """A git repository on `main` with an identity of its own."""
    root.mkdir()
    git(root, "init", "-q", "-b", "main")
    identify(root)


def identify(root: Path) -> None:
    """Give the repository at `root` an identity of its own."""
    git(root, "config", "user.name", "t")
    git(root, "config", "user.email", "t@t")


def commit(root: Path) -> str:
    """Commit everything in `root` and return the commit."""
    git(root, "add", ".")
    git(root, "commit", "-qm", "fixture")
    return git(root, "rev-parse", "HEAD")


def mount(vaults: Vaults, name: str, root: Path) -> None:
    """Mount the vault cloned at `root` at /vaults/<name> in the board, pinned to its HEAD, and commit it."""
    pointer = {"$type": "board/mount/v1", "repo": str(root), "follow": "main", "commit": git(root, "rev-parse", "HEAD")}
    vaults.write(vaults.board, f"vaults/{name}.json", json.dumps(pointer))
    commit(vaults.board)


def vault(root: Path, remote: Path) -> None:
    """A vault in Reflect's layout, a board itself, at `root`, pushed to the bare `remote`."""
    git(remote.parent, "init", "-q", "--bare", "-b", "main", str(remote))
    repository(root)
    (root / ".board").mkdir()
    (root / ".board" / "board.json").write_text(json.dumps({"$type": "board/config/v1"}))
    (root / ".gitignore").write_text(".reflect/\n")
    for directory in ("daily", "notes", "assets"):
        (root / directory).mkdir()
        (root / directory / ".gitkeep").write_text("")
    commit(root)
    git(root, "remote", "add", "origin", str(remote))
    git(root, "push", "-qu", "origin", "main")


@pytest.fixture
def vaults(tmp_path: Path) -> Vaults:
    """A board enabling notes that mounts one vault, its remote and a phone's clone of it."""
    fixture = Vaults(
        board=tmp_path / "board",
        vault=tmp_path / "vault",
        remote=tmp_path / "remote.git",
        phone=tmp_path / "phone",
        data=tmp_path / "data",
        cache=tmp_path / "cache",
    )
    vault(fixture.vault, fixture.remote)
    git(tmp_path, "clone", "-q", str(fixture.remote), str(fixture.phone))
    identify(fixture.phone)
    repository(fixture.board)
    config = {"$type": "board/config/v1", "plugins": ["notes"]}
    fixture.write(fixture.board, ".board/board.json", json.dumps(config))
    mount(fixture, "personal", fixture.vault)
    plugins = fixture.data / "board" / "plugins"
    plugins.mkdir(parents=True)
    (plugins / "notes").symlink_to(PLUGIN)
    return fixture
