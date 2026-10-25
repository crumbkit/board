"""A board that mounts a notes repo at /repos/notes-personal, the repo's remote, and a phone that pushes to it.

Each is a real git repository. The clone is the one Reflect's desktop app
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
SCHEMA = PLUGIN / "types" / "notes" / "repo" / "v1.schema.json"


def board_binary() -> Path:
    """The `board` under test: `$BOARD_BIN`, or the workspace's debug build."""
    return Path(os.environ.get("BOARD_BIN", REPOSITORY / "target" / "debug" / "board")).resolve()


def git(root: Path, *arguments: str) -> str:
    """Run git in `root` and return what it printed, stripped."""
    ran = subprocess.run(
        ["git", "-c", "commit.gpgsign=false", *arguments], cwd=root, capture_output=True, text=True, check=True
    )
    return ran.stdout.strip()


def dates(zone: str, start: dt.timedelta | None = None) -> set[str]:
    """The person's day in `zone`, for a day starting `start` after midnight, read now and an hour on.

    A call made between the two readings is on one of the two days.
    """
    now = dt.datetime.now(ZoneInfo(zone))
    return {(moment - (start or dt.timedelta())).date().isoformat() for moment in (now, now + dt.timedelta(hours=1))}


@dataclass(frozen=True)
class Notes:
    """`board`, which mounts `clone` at /repos/notes-personal; `remote`, which `clone` and `phone` clone."""

    board: Path
    clone: Path
    remote: Path
    phone: Path
    data: Path
    cache: Path
    config: Path

    def write(self, root: Path, path: str, text: str) -> None:
        """Write a fixture file at `path` under `root`. Every test writes through here."""
        target = root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text)

    def run(self, *arguments: str, tz: str | None = None, cwd: Path | None = None) -> subprocess.CompletedProcess[str]:
        """Run `board` on the board, with only the variables a user's shell would pass it, and `TZ` when given.

        From `cwd`, a caller's checkout, it runs `board -C <board>`; otherwise in the board.
        """
        uv = shutil.which("uv")
        assert uv is not None, "uv is on PATH"
        path = os.pathsep.join([str(board_binary().parent), str(Path(uv).parent), "/usr/bin", "/bin"])
        environment = {
            "PATH": path,
            "HOME": os.environ.get("HOME", str(self.board)),
            "XDG_DATA_HOME": str(self.data),
            "XDG_CACHE_HOME": str(self.cache),
            "XDG_CONFIG_HOME": str(self.config),
        } | ({"TZ": tz} if tz else {})
        return subprocess.run(
            [str(board_binary()), *(["-C", str(self.board)] if cwd else []), *arguments],
            cwd=cwd or self.board,
            env=environment,
            capture_output=True,
            text=True,
            check=False,
        )

    def call(
        self, capability: str, *flags: str, tz: str | None = None, cwd: Path | None = None
    ) -> tuple[int, dict[str, Any]]:
        """`board --json call notes.<capability> <flags>`: its exit code, and its output or failure."""
        ran = self.run("--json", "call", f"notes.{capability}", *flags, tz=tz, cwd=cwd)
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
        """The path in the notes repo of the note a call's `output` names."""
        return str(output["path"]).removeprefix("/repos/notes-personal/")

    def pin(self) -> str:
        """The commit the board's mount of the notes repo reads."""
        pointer: dict[str, str] = json.loads((self.board / "repos" / "notes-personal.json").read_text())
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


def mount(notes: Notes, name: str, root: Path) -> None:
    """Mount the repository cloned at `root` at /repos/<name> in the board, pinned to its HEAD, and commit it."""
    pointer = {"$type": "board/mount/v1", "repo": str(root), "follow": "main", "commit": git(root, "rev-parse", "HEAD")}
    notes.write(notes.board, f"repos/{name}.json", json.dumps(pointer))
    commit(notes.board)


def layout(root: Path) -> None:
    """Reflect's layout in `root`: daily/, notes/ and assets/, each kept by a `.gitkeep`."""
    for directory in ("daily", "notes", "assets"):
        (root / directory).mkdir(parents=True, exist_ok=True)
        (root / directory / ".gitkeep").write_text("")


def notes_repo(root: Path, remote: Path) -> None:
    """A notes repo in Reflect's layout, a board itself, at `root`, pushed to the bare `remote`."""
    git(remote.parent, "init", "-q", "--bare", "-b", "main", str(remote))
    repository(root)
    (root / ".board").mkdir()
    (root / ".board" / "board.json").write_text(json.dumps({"$type": "board/config/v1"}))
    (root / ".gitignore").write_text(".reflect/\n")
    layout(root)
    # Repos with the same tree, author and time would share a commit, which the index lists under one mount.
    (root / "repo.txt").write_text(f"{root.name}\n")
    commit(root)
    git(root, "remote", "add", "origin", str(remote))
    git(root, "push", "-qu", "origin", "main")


def cover(notes: Notes, root: Path, covers: list[str], *, personal: bool = False) -> None:
    """Give the notes repo cloned at `root` its `notes/repo/v1` record and that type's schema.

    It commits and pushes them, and pins the mount at /repos/<root's name> to them, as a person would after
    writing the record, since a call reads every other repo's record at its mount's pin.
    """
    record: dict[str, object] = {"$type": "notes/repo/v1", "covers": covers}
    if personal:
        record["personal"] = True
    (root / "notes-repo.json").write_text(json.dumps(record))
    types = root / ".board" / "types" / "notes" / "repo"
    types.mkdir(parents=True, exist_ok=True)
    shutil.copy(SCHEMA, types / "v1.schema.json")
    commit(root)
    git(root, "push", "-q")
    pinned = notes.run("mount", "update", f"/repos/{root.name}")
    assert pinned.returncode == 0, pinned.stderr


def checkout(notes: Notes, origin: str) -> Path:
    """A caller's checkout, outside the board, whose origin is `origin`."""
    root = notes.board.parent / f"checkout-{len(list(notes.board.parent.glob('checkout-*')))}"
    repository(root)
    git(root, "remote", "add", "origin", origin)
    return root


def another(notes: Notes, name: str) -> Path:
    """Another notes repo with a remote of its own, mounted at /repos/<name>: its clone."""
    clone = notes.board.parent / name
    notes_repo(clone, notes.board.parent / f"{name}.git")
    mount(notes, name, clone)
    return clone


@pytest.fixture
def notes(tmp_path: Path) -> Notes:
    """A board enabling notes that mounts one notes repo, its remote and a phone's clone of it."""
    fixture = Notes(
        board=tmp_path / "board",
        clone=tmp_path / "notes-personal",
        remote=tmp_path / "remote.git",
        phone=tmp_path / "phone",
        data=tmp_path / "data",
        cache=tmp_path / "cache",
        config=tmp_path / "config",
    )
    notes_repo(fixture.clone, fixture.remote)
    git(tmp_path, "clone", "-q", str(fixture.remote), str(fixture.phone))
    identify(fixture.phone)
    repository(fixture.board)
    fixture.write(fixture.board, ".board/board.json", json.dumps({"$type": "board/config/v1"}))
    commit(fixture.board)
    plugins = fixture.data / "board" / "plugins"
    plugins.mkdir(parents=True)
    (plugins / "notes").symlink_to(PLUGIN)
    enabled = fixture.run("plugin", "enable", "notes")
    assert enabled.returncode == 0, enabled.stderr
    mount(fixture, "notes-personal", fixture.clone)
    return fixture
