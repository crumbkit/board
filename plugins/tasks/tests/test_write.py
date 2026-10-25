"""The write calls through the real `board`: add, done, defer, drop and reviewed, in the board's tree and a mount.

Dates are counted from the real today. test_lines.py and test_repeat.py hold the line editor and the repeat
rules.
"""

import datetime as dt
import json
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any
from zoneinfo import ZoneInfo

import pytest
from board_plugin.board import Board as PluginBoard
from board_plugin.protocol import BoardContext, CallRequest, PluginError
from board_plugin.repos import File, Repo
from board_plugin.writing import HEAD_MOVED, STALE
from conftest import Board, git, notes_repo, task_v2, zone_at_one_in_the_morning

from board_tasks import write

TODAY = dt.date.today()  # noqa: DTZ011 - the plugin reads the local day too
NOT_SYNCED = "tasks::repo::not_synced"
EXAMPLE_ORG = "git@github.com:example-org/home.git"


def call(
    board: Board, capability: str, *flags: str, tz: str | None = None, stdin: str = ""
) -> tuple[int, dict[str, Any]]:
    """`board --json call tasks.<capability> <flags>` from outside any git repository: the exit code and output."""
    outside = board.root.parent / "outside"
    outside.mkdir(exist_ok=True)
    ran = board.run("--json", "call", f"tasks.{capability}", *flags, cwd=outside, tz=tz, stdin=stdin)
    document: dict[str, Any] = json.loads(ran.stdout)
    return ran.returncode, document["data"]["output"] if ran.returncode == 0 else document


def day(offset: int = 0) -> str:
    """Today, or `offset` days from it, as YYYY-MM-DD."""
    return (TODAY + dt.timedelta(days=offset)).isoformat()


@pytest.fixture
def mounted(board: Board) -> Path:
    """The board, committing puts, with a personal notes repo and a work one covering example-org/*: the work clone."""
    config = json.loads((board.root / ".board" / "board.json").read_text()) | {"commit_on_put": True}
    board.write(".board/board.json", json.dumps(config))
    git(board.root, "config", "user.name", "t")
    git(board.root, "config", "user.email", "t@t")
    notes_repo(board, "personal", {"covers": [], "personal": True}, {})
    return notes_repo(board, "work", {"covers": ["example-org/*"]}, {"report": task_v2("Report", reviewed=day(-30))})


def test_add_writes_an_open_task_named_by_its_title_reviewed_today_and_a_second_one_beside_it(board: Board) -> None:
    code, output = call(board, "add", "--title", "Call the bank", "--defer", "soon", "--text", "Ask about fees.")
    assert code == 0, output
    assert output == {
        "repo": "/",
        "path": "/tasks/call-the-bank.md",
        "status": "open",
        "defer": day(20),
        "reviewed": day(),
        "commit": None,
        "unsynced": NOT_SYNCED,
        "unpushed": NOT_SYNCED,
    }
    assert (board.root / "tasks" / "call-the-bank.md").read_text() == (
        f'---\n$type: "tasks/task/v2"\ntitle: "Call the bank"\nstatus: "open"\ndefer: "{day(20)}"\n'
        f'reviewed: "{day()}"\n---\n\nAsk about fees.\n'
    )
    code, again = call(board, "add", stdin=json.dumps({"title": "Call the bank", "contexts": ["phone"]}))
    assert (code, again["path"], again["defer"]) == (0, "/tasks/call-the-bank-2.md", None), again
    listed = board.json("call", "tasks.available")["data"]["output"]
    assert [task["path"] for task in listed["available"]["tasks"]] == ["/tasks/call-the-bank-2.md"]


def test_add_goes_to_the_repo_covering_the_remote_and_pushes_and_outside_one_to_the_personal_repo(
    board: Board, mounted: Path
) -> None:
    code, output = call(board, "add", "--title", "Ship it", "--remote", EXAMPLE_ORG)
    assert code == 0, output
    assert (output["repo"], output["path"]) == ("/repos/work", "/repos/work/tasks/ship-it.md")
    assert (output["unsynced"], output["unpushed"]) == (None, None)
    assert output["commit"] == git(mounted, "rev-parse", "HEAD") == git(mounted, "rev-parse", "origin/main")
    assert "tasks/ship-it.md" in git(mounted, "ls-tree", "-r", "--name-only", "origin/main")
    code, personal = call(board, "add", "--title", "Buy milk")
    assert (code, personal["path"]) == (0, "/repos/personal/tasks/buy-milk.md"), personal


def test_add_refuses_a_remote_two_repos_cover_and_writes_to_one_named_with_repo(board: Board, mounted: Path) -> None:
    notes_repo(board, "more", {"covers": ["example-org/home"]}, {})
    code, refused = call(board, "add", "--title", "Ship it", "--remote", EXAMPLE_ORG)
    assert (code, refused["code"]) == (1, "tasks::repo::ambiguous"), refused
    assert refused["fields"]["candidates"] == ["/repos/more", "/repos/work"]
    assert not (mounted / "tasks" / "ship-it.md").exists()
    code, output = call(board, "add", "--title", "Ship it", "--remote", EXAMPLE_ORG, "--repo", "/repos/work")
    assert (code, output["path"]) == (0, "/repos/work/tasks/ship-it.md"), output
    code, refused = call(board, "add", "--title", "Ship it", "--repo", "/repos/elsewhere")
    assert (code, refused["code"]) == (1, "tasks::repo::not_found"), refused


def test_done_sets_status_and_reviewed_and_keeps_every_other_byte(board: Board) -> None:
    text = '---\n$type: tasks/task/v2\ntitle: Pay rent  # monthly\nstatus: open\nid: "r1"\n---\n\nBody.\n'
    board.write("tasks/rent.md", text)
    code, output = call(board, "done", "--path", "/tasks/rent.md")
    assert code == 0, output
    assert (output["status"], output["reviewed"], output["commit"]) == ("done", day(), None)
    assert (board.root / "tasks" / "rent.md").read_text() == text.replace(
        'status: open\nid: "r1"\n', f'status: "done"\nid: "r1"\nreviewed: "{day()}"\n'
    )


def test_done_defers_a_repeating_task_to_its_next_occurrence_and_it_stays_open(board: Board) -> None:
    board.write("tasks/water.md", task_v2("Water the plants", repeat="daily", defer=day(-3)))
    board.write("tasks/stretch.md", task_v2("Stretch", repeat="daily", ephemeral=True, defer=day(-3)))
    code, output = call(board, "done", "--path", "/tasks/water.md")
    assert (code, output["status"], output["defer"]) == (0, "open", day(-2)), output
    code, output = call(board, "done", "--path", "/tasks/stretch.md")
    assert (code, output["status"], output["defer"]) == (0, "open", day(1)), output


def test_a_finished_task_is_refused_by_done_defer_and_drop_and_admitted_by_reviewed(board: Board) -> None:
    board.write("tasks/old.md", task_v2("Old", status="done"))
    for capability, *flags in (("done",), ("drop",), ("defer", "--until", "soon")):
        code, refused = call(board, capability, "--path", "/tasks/old.md", *flags)
        assert (code, refused["code"]) == (1, "tasks::task::finished"), (capability, refused)
        assert (refused["fields"]["path"], refused["fields"]["status"]) == ("/tasks/old.md", "done")
    code, output = call(board, "reviewed", "--path", "/tasks/old.md")
    assert (code, output["status"], output["reviewed"]) == (0, "done", day()), output


def test_defer_writes_a_horizon_as_its_date_and_a_date_as_it_is_and_drop_drops(board: Board) -> None:
    board.write("tasks/a.md", task_v2("A"))
    assert call(board, "defer", "--path", "/tasks/a.md", "--until", "not-this-week")[1]["defer"] == day(6)
    assert call(board, "defer", "--path", "/tasks/a.md", "--until", day(40))[1]["defer"] == day(40)
    assert f'defer: "{day(40)}"' in (board.root / "tasks" / "a.md").read_text()
    code, output = call(board, "drop", "--path", "/tasks/a.md")
    assert (code, output["status"]) == (0, "dropped"), output


def test_a_path_holding_no_task_is_refused_and_a_task_beside_it_is_admitted(board: Board) -> None:
    board.write("tasks/note.md", "# A note, not a task\n")
    board.write("notes/elsewhere.md", task_v2("Elsewhere"))
    board.write("tasks/fortnightly.md", task_v2("Fortnightly", repeat="fortnightly"))
    board.write("tasks/real.md", task_v2("Real", repeat="weekly-mon"))
    for path, refusal in (
        ("/tasks/gone.md", "tasks::task::not_found"),
        ("/tasks/note.md", "tasks::task::not_task"),
        ("/notes/elsewhere.md", "tasks::task::not_task"),
        ("/tasks/fortnightly.md", "tasks::task::not_task"),
    ):
        code, refused = call(board, "done", "--path", path)
        assert (code, refused["code"], refused["fields"]["path"]) == (1, refusal, path), refused
    assert "`board check /tasks/fortnightly.md`" in refused["message"], refused
    assert call(board, "done", "--path", "/tasks/real.md")[0] == 0


def test_done_in_a_mount_pushes_the_commit_and_moves_the_pin(board: Board, mounted: Path) -> None:
    code, output = call(board, "done", "--path", "/repos/work/tasks/report.md")
    assert code == 0, output
    head = git(mounted, "rev-parse", "HEAD")
    assert (output["repo"], output["commit"], output["unpushed"]) == ("/repos/work", head, None)
    assert git(mounted, "rev-parse", "origin/main") == head
    assert json.loads((board.root / "repos" / "work.json").read_text())["commit"] == head
    assert 'status: "done"' in git(mounted, "show", "HEAD:tasks/report.md")


def test_a_write_after_a_sync_that_cannot_reach_the_remote_is_committed_and_the_next_call_pushes_it(
    board: Board, mounted: Path
) -> None:
    remote = board.root.parent / "work.git"
    remote.rename(remote.with_name("away.git"))
    code, output = call(board, "done", "--path", "/repos/work/tasks/report.md")
    assert code == 0, output
    assert output["unsynced"] == "board::sync::failed", output
    assert output["commit"] == git(mounted, "rev-parse", "HEAD") != git(mounted, "rev-parse", "origin/main")
    remote.with_name("away.git").rename(remote)
    code, output = call(board, "reviewed", "--path", "/repos/work/tasks/report.md")
    assert (code, output["unsynced"], output["unpushed"]) == (0, None, None), output
    assert git(remote, "show", "main:tasks/report.md").count('status: "done"') == 1


@pytest.mark.usefixtures("mounted")
def test_a_task_in_a_mount_that_is_not_a_notes_repo_is_refused_and_one_in_a_notes_repo_is_admitted(
    board: Board,
) -> None:
    plain = notes_repo(board, "plain", None, {"chore": task_v2("Chore")}, daily=False)
    code, refused = call(board, "done", "--path", "/repos/plain/tasks/chore.md")
    assert (code, refused["code"]) == (1, "tasks::repo::not_found"), refused
    assert refused["fields"]["path"] == "/repos/plain/tasks/chore.md"
    assert git(plain, "status", "--porcelain") == ""
    assert git(plain, "rev-list", "--count", "HEAD") == "1", "nothing was committed in the plain clone"
    code, output = call(board, "done", "--path", "/repos/work/tasks/report.md")
    assert (code, output["repo"]) == (0, "/repos/work"), output


def test_a_push_that_fails_after_the_write_is_reported_and_the_write_stays_committed(
    board: Board, mounted: Path
) -> None:
    hook = board.root.parent / "work.git" / "hooks" / "pre-receive"
    hook.write_text("#!/bin/sh\nexit 1\n")
    hook.chmod(0o755)
    code, output = call(board, "done", "--path", "/repos/work/tasks/report.md")
    assert code == 0, output
    assert output["unpushed"] is not None, output
    assert output["commit"] == git(mounted, "rev-parse", "HEAD") != git(mounted, "rev-parse", "origin/main")
    assert 'status: "done"' in git(mounted, "show", "HEAD:tasks/report.md")


def test_add_inside_a_git_repository_without_origin_is_refused_and_admitted_with_remote(
    board: Board, mounted: Path
) -> None:
    checkout = board.root.parent / "checkout"
    git(board.root.parent, "init", "-q", str(checkout))
    ran = board.run("--json", "call", "tasks.add", "--title", "Ship it", cwd=checkout)
    refused = json.loads(ran.stdout)
    assert (ran.returncode, refused["code"]) == (1, "tasks::repo::origin_unread"), refused
    assert refused["fields"]["origin_unread"].startswith(f"git gave no origin for {checkout.resolve()}: ")
    assert not list((mounted.parent / "personal").glob("tasks/*.md"))
    ran = board.run("--json", "call", "tasks.add", "--title", "Ship it", "--remote", EXAMPLE_ORG, cwd=checkout)
    output = json.loads(ran.stdout)["data"]["output"]
    assert (ran.returncode, output["path"]) == (0, "/repos/work/tasks/ship-it.md"), output


def test_a_write_before_the_configured_day_start_is_reviewed_on_the_day_before(board: Board) -> None:
    zone = zone_at_one_in_the_morning()
    local = dt.datetime.now(ZoneInfo(zone)).date()
    board.write("tasks/a.md", task_v2("A"))
    (board.config / "board").mkdir(parents=True)
    (board.config / "board" / "config.json").write_text(json.dumps({"day_start": "07:00"}))
    output = call(board, "defer", "--path", "/tasks/a.md", "--until", "not-today", tz=zone)[1]
    assert (output["reviewed"], output["defer"]) == ((local - dt.timedelta(days=1)).isoformat(), local.isoformat())
    (board.config / "board" / "config.json").write_text("{}")
    assert call(board, "reviewed", "--path", "/tasks/a.md", tz=zone)[1]["reviewed"] == local.isoformat()


def test_a_task_with_an_edit_not_committed_in_its_clone_is_busy_and_admitted_once_the_edit_is_gone(
    board: Board, mounted: Path
) -> None:
    report = mounted / "tasks" / "report.md"
    report.write_text(report.read_text() + "An edit on disk.\n")
    code, refused = call(board, "done", "--path", "/repos/work/tasks/report.md")
    assert (code, refused["code"]) == (1, "tasks::task::busy"), refused
    assert refused["fields"]["code"] == "board::mount::stale"
    assert "call again" in refused["message"], refused
    assert report.read_text().endswith("An edit on disk.\n")
    git(mounted, "checkout", "--", "tasks/report.md")
    code, output = call(board, "done", "--path", "/repos/work/tasks/report.md")
    assert (code, output["status"], output["unsynced"]) == (0, "done", None), output


@dataclass(frozen=True)
class Racing(Repo):
    """A mount whose puts Board refuses with `codes` in turn, then admits, holding the text each put sent.

    A stale put writes nothing. A put whose HEAD moved writes its text before it is refused.
    """

    codes: tuple[str, ...] = ()
    texts: list[str] = field(default_factory=list[str])
    puts: list[str] = field(default_factory=list[str])

    def pin(self) -> str | None:
        """A pin that never moves."""
        return None

    def sync(self, *, push: bool) -> str | None:
        """A sync that finds nothing to do."""
        del push
        return None

    def read(self, path: str) -> File:
        """The task, as the last put that wrote left it."""
        del path
        text = self.texts[-1] if self.texts else task_v2("Report")
        record = {"$type": "tasks/task/v2", "title": "Report", "status": "done" if "done" in text else "open"}
        return File(text, str(len(self.texts)), record | {"reviewed": day()}, {"repository": self.clone})

    def write(self, path: str, text: str, expect: str | None) -> str | None:
        """Refuse with the next code, writing the text for a moved HEAD, else write it and commit."""
        del expect
        self.puts.append(path)
        code = self.codes[len(self.puts) - 1] if len(self.puts) <= len(self.codes) else None
        if code != STALE:
            self.texts.append(text)
        if code is not None:
            raise PluginError("board::put::refused", "refused", {"code": code})
        return "c1"


def racing(monkeypatch: pytest.MonkeyPatch, *codes: str) -> Racing:
    """A `Racing` mount that `holder` returns for every path."""
    repo = Racing(PluginBoard("/board", "tasks"), "/repos/work", "/clone", codes)

    def holder(board: PluginBoard, logical: str) -> Repo:
        del board, logical
        return repo

    monkeypatch.setattr(write, "holder", holder)
    return repo


REQUEST = CallRequest("done", {"path": "/repos/work/tasks/report.md"}, BoardContext("/board", "/db"))


def test_a_stale_put_is_tried_again_on_a_new_read_and_a_landed_one_is_not_written_twice(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    stale = racing(monkeypatch, STALE)
    output = write.done(REQUEST)
    assert (output["status"], output["commit"], len(stale.puts), len(stale.texts)) == ("done", "c1", 2, 1), output
    moved = racing(monkeypatch, HEAD_MOVED)
    output = write.done(REQUEST)
    assert (output["status"], output["commit"], len(moved.puts)) == ("done", None, 1), output
    busy = racing(monkeypatch, STALE, STALE, STALE)
    with pytest.raises(PluginError) as refused:
        write.done(REQUEST)
    assert (refused.value.code, len(busy.puts), busy.texts) == ("tasks::task::busy", 3, [])
