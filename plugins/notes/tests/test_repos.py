"""How a call finds notes repos, routes a note to one, and dates it by the person's day, through the real `board`."""

import datetime as dt
import json
import re
from dataclasses import dataclass, field
from zoneinfo import ZoneInfo

import pytest
from board_plugin.board import Board
from board_plugin.protocol import PluginError
from conftest import Notes, another, checkout, commit, cover, dates, git, layout, mount, notes_repo
from pydantic import ValidationError

from board_notes.models import RepoRecord
from board_notes.notes import ATTEMPTS, HEAD_MOVED, INBOX_NAMES, STALE, append, create
from board_notes.repo import Note, Repo

EXAMPLE_ORG = "https://github.com/example-org/home.git"
VIITTA = "git@github.com:crumbkit/viitta.git"


def test_a_mount_without_daily_is_not_a_notes_repo_and_one_with_it_is(notes: Notes) -> None:
    plain = notes.board.parent / "plain"
    notes_repo(plain, notes.board.parent / "plain.git")
    git(plain, "rm", "-rq", "daily")
    git(plain, "commit", "-qm", "no daily")
    mount(notes, "plain", plain)
    code, refused = notes.call("daily", "--repo", "/repos/plain")
    assert (code, refused["code"]) == (1, "notes::repo::not_found")
    assert refused["fields"]["repos"] == ["/repos/notes-personal"]
    code, read = notes.call("daily", "--repo", "/repos/notes-personal")
    assert code == 0, read
    code, only = notes.call("daily")
    assert (code, only["repo"]) == (0, "/repos/notes-personal"), "the only notes repo needs no --repo"


def test_a_board_without_a_notes_repo_is_refused_and_one_with_its_own_daily_is_its_notes_repo(notes: Notes) -> None:
    git(notes.board, "rm", "-q", "repos/notes-personal.json")
    git(notes.board, "commit", "-qm", "unmount")
    code, refused = notes.call("add", "--text", "Lost.")
    assert (code, refused["code"]) == (1, "notes::repo::not_found")
    assert refused["fields"]["repos"] == []
    layout(notes.board)
    commit(notes.board)
    code, output = notes.call("add", "--text", "Agents only.")
    assert code == 0, output
    head = git(notes.board, "rev-parse", "HEAD")
    assert (output["repo"], output["commit"], output["unpushed"]) == ("/", head, "notes::repo::not_synced")
    assert re.fullmatch(r"/daily/\d{4}-\d\d-\d\d\.md", output["path"]), output["path"]
    assert git(notes.board, "show", "--name-only", "--format=", head) == output["path"].removeprefix("/")
    code, read = notes.call("daily", "--repo", "/")
    assert code == 0, read
    assert (read["path"], read["commit"], read["unsynced"]) == (output["path"], None, "notes::repo::not_synced")
    assert str(read["text"]).endswith("Agents only.\n")


def test_a_note_goes_to_the_one_repo_covering_the_remote_and_otherwise_to_the_personal_inbox(notes: Notes) -> None:
    work = another(notes, "notes-work")
    cover(notes, work, ["Example-Org/*"])
    cover(notes, notes.clone, ["crumbkit/board"], personal=True)
    code, routed = notes.call("add", "--text", "Deploy.", "--remote", EXAMPLE_ORG)
    assert code == 0, routed
    assert (routed["repo"], routed["candidates"]) == ("/repos/notes-work", None)
    assert routed["path"].startswith("/repos/notes-work/daily/")
    code, inboxed = notes.call("add", "--text", "Rename.", "--remote", VIITTA, "--topic", "Board: names")
    assert code == 0, inboxed
    assert (inboxed["repo"], inboxed["candidates"]) == ("/repos/notes-personal", [])
    path = notes.note(inboxed)
    assert re.fullmatch(r"inbox/\d{4}-\d\d-\d\d-\d{6}\.md", path), path
    frontmatter = '---\nremote: "crumbkit/viitta"\ncandidates: []\ntopic: "Board: names"\n---\n\n'
    text = git(notes.remote, "show", f"main:{path}")
    assert re.fullmatch(re.escape(frontmatter) + r"## \d\d:\d\d\n\nRename\.", text), text


def test_a_note_two_repos_cover_goes_to_the_inbox_naming_both_and_one_repo_covering_it_takes_it(notes: Notes) -> None:
    work = another(notes, "notes-work")
    cover(notes, work, ["example-org/*"])
    cover(notes, notes.clone, [], personal=True)
    code, output = notes.call("add", "--text", "Mine.", "--remote", "example-org/home")
    assert (code, output["repo"]) == (0, "/repos/notes-work"), output
    home = another(notes, "notes-home")
    cover(notes, home, ["example-org/home"])
    code, output = notes.call("add", "--text", "Whose?", "--remote", "example-org/home")
    assert code == 0, output
    both = ["/repos/notes-home", "/repos/notes-work"]
    assert (output["repo"], output["candidates"]) == ("/repos/notes-personal", both)
    assert 'candidates: ["/repos/notes-home", "/repos/notes-work"]' in (notes.clone / notes.note(output)).read_text()


def test_without_a_personal_repo_an_uncovered_note_is_refused_and_with_one_it_goes_to_the_inbox(notes: Notes) -> None:
    another(notes, "notes-work")
    head = git(notes.clone, "rev-parse", "HEAD")
    code, refused = notes.call("add", "--text", "Where?")
    assert (code, refused["code"]) == (1, "notes::repo::no_personal")
    assert refused["fields"]["personal"] == []
    assert git(notes.clone, "rev-parse", "HEAD") == head, "nothing was written"
    cover(notes, notes.clone, [], personal=True)
    code, output = notes.call("add", "--text", "Where?")
    assert code == 0, output
    assert notes.note(output).startswith("inbox/")
    assert "remote:" not in (notes.clone / notes.note(output)).read_text(), "no remote was given"


def test_daily_reads_the_note_of_the_repo_covering_the_remote_and_refuses_one_no_repo_covers(notes: Notes) -> None:
    work = another(notes, "notes-work")
    cover(notes, work, ["example-org/*"])
    cover(notes, notes.clone, [], personal=True)
    code, covered = notes.call("daily", "--remote", EXAMPLE_ORG)
    assert (code, covered["repo"]) == (0, "/repos/notes-work"), covered
    code, refused = notes.call("daily", "--remote", VIITTA)
    assert (code, refused["code"]) == (1, "notes::repo::ambiguous"), "the personal note is never read by default"
    assert refused["fields"]["candidates"] == []
    code, named = notes.call("daily", "--remote", VIITTA, "--repo", "/repos/notes-personal")
    assert (code, named["repo"]) == (0, "/repos/notes-personal"), named


def test_daily_without_a_remote_reads_the_personal_note_and_one_with_an_uncovered_remote_is_refused(
    notes: Notes,
) -> None:
    work = another(notes, "notes-work")
    cover(notes, work, ["example-org/*"])
    cover(notes, notes.clone, [], personal=True)
    code, personal = notes.call("daily")
    assert (code, personal["repo"]) == (0, "/repos/notes-personal"), personal
    code, refused = notes.call("daily", cwd=checkout(notes, VIITTA))
    assert (code, refused["code"]) == (1, "notes::repo::ambiguous"), refused


def test_the_only_repo_covering_something_refuses_a_call_without_a_remote_and_takes_one_it_covers(notes: Notes) -> None:
    cover(notes, notes.clone, ["example-org/*"])
    code, refused = notes.call("daily")
    assert (code, refused["code"]) == (1, "notes::repo::ambiguous"), "no remote may be a missing origin"
    code, refused = notes.call("add", "--text", "Where?")
    assert (code, refused["code"]) == (1, "notes::repo::no_personal"), refused
    code, daily = notes.call("daily", "--remote", EXAMPLE_ORG)
    assert (code, daily["repo"]) == (0, "/repos/notes-personal"), daily


def test_the_only_repo_without_a_record_serves_a_call_without_a_remote_and_refuses_one_with_a_remote(
    notes: Notes,
) -> None:
    code, daily = notes.call("daily")
    assert (code, daily["repo"]) == (0, "/repos/notes-personal"), daily
    code, refused = notes.call("daily", "--remote", EXAMPLE_ORG)
    assert (code, refused["code"]) == (1, "notes::repo::ambiguous"), "a repo without a record covers nothing"
    code, refused = notes.call("add", "--text", "Where?", "--remote", EXAMPLE_ORG)
    assert (code, refused["code"]) == (1, "notes::repo::no_personal"), refused


def test_a_remote_s_credential_is_never_stored_or_shown_and_its_owner_and_name_are(notes: Notes) -> None:
    tokened = "https://x-access-token:SECRET@github.com/crumbkit/viitta.git"
    another(notes, "notes-work")
    code, refused = notes.call("add", "--text", "Where?", "--remote", tokened)
    assert (code, refused["code"]) == (1, "notes::repo::no_personal")
    assert "SECRET" not in json.dumps(refused)
    assert "crumbkit/viitta" in refused["message"]
    cover(notes, notes.clone, [], personal=True)
    code, output = notes.call("add", "--text", "Where?", "--remote", tokened)
    assert code == 0, output
    text = (notes.clone / notes.note(output)).read_text()
    assert ("SECRET" in text, 'remote: "crumbkit/viitta"' in text) == (False, True)


def test_a_note_is_routed_by_the_origin_of_the_directory_board_runs_in_and_remote_overrides_it(notes: Notes) -> None:
    work = another(notes, "notes-work")
    cover(notes, work, ["example-org/*"])
    cover(notes, notes.clone, [], personal=True)
    home = checkout(notes, EXAMPLE_ORG)
    code, routed = notes.call("add", "--text", "From home.", cwd=home)
    assert (code, routed["repo"], routed["candidates"]) == (0, "/repos/notes-work", None), routed
    code, overridden = notes.call("add", "--text", "Elsewhere.", "--remote", VIITTA, cwd=home)
    assert (code, overridden["repo"], overridden["candidates"]) == (0, "/repos/notes-personal", []), overridden
    assert 'remote: "crumbkit/viitta"' in (notes.clone / notes.note(overridden)).read_text()


def test_the_only_notes_repo_takes_a_note_it_covers_and_refuses_one_its_record_excludes(notes: Notes) -> None:
    cover(notes, notes.clone, ["example-org/*"])
    head = git(notes.clone, "rev-parse", "HEAD")
    code, refused = notes.call("add", "--text", "Not for work.", cwd=checkout(notes, VIITTA))
    assert (code, refused["code"]) == (1, "notes::repo::no_personal")
    assert git(notes.clone, "rev-parse", "HEAD") == head, "nothing was written"
    code, output = notes.call("add", "--text", "For work.", cwd=checkout(notes, EXAMPLE_ORG))
    assert (code, output["repo"], output["candidates"]) == (0, "/repos/notes-personal", None), output


def test_a_record_without_its_schema_is_refused_by_name_and_one_with_it_is_admitted(notes: Notes) -> None:
    another(notes, "notes-work")
    notes.write(notes.clone, "notes-repo.json", json.dumps({"$type": "notes/repo/v1", "covers": [], "personal": True}))
    commit(notes.clone)
    pinned = notes.run("mount", "update", "/repos/notes-personal")
    assert pinned.returncode == 0, pinned.stderr
    code, refused = notes.call("add", "--text", "Where?")
    assert (code, refused["code"]) == (1, "notes::repo::invalid_record")
    assert refused["fields"]["path"] == "/repos/notes-personal/notes-repo"
    assert refused["fields"]["code"] == "board::record::unknown_type"
    assert "plugin enable notes" in refused["message"]
    cover(notes, notes.clone, [], personal=True)
    code, output = notes.call("add", "--text", "Where?")
    assert (code, output["repo"]) == (0, "/repos/notes-personal"), output


def test_an_inbox_note_is_written_whole_when_the_clone_cannot_sync_and_a_daily_note_is_parked(notes: Notes) -> None:
    another(notes, "notes-work")
    cover(notes, notes.clone, [], personal=True)
    path = "daily/2020-01-01.md"
    notes.write(notes.clone, path, "Old.\n")
    commit(notes.clone)
    git(notes.clone, "push", "-q")
    notes.phone_appends(path, "From the phone.\n")
    notes.write(notes.clone, path, "Old.\nTyping in Reflect")
    code, inboxed = notes.call("add", "--text", "Whose?", "--remote", VIITTA, "--topic", "T")
    assert code == 0, inboxed
    assert (inboxed["parked"], inboxed["unpushed"]) == (None, "board::sync::edited")
    assert notes.note(inboxed).startswith("inbox/")
    frontmatter = '---\nremote: "crumbkit/viitta"\ncandidates: []\ntopic: "T"\n---\n\n'
    assert (notes.clone / notes.note(inboxed)).read_text().startswith(frontmatter)
    code, parked = notes.call("add", "--text", "Mine.", "--repo", "/repos/notes-personal")
    assert (code, parked["parked"]) == (0, "board::sync::edited"), parked


def test_two_inbox_notes_in_one_second_are_two_files(notes: Notes) -> None:
    another(notes, "notes-work")
    cover(notes, notes.clone, [], personal=True)
    now = dt.datetime.now(dt.UTC)
    taken = [f"inbox/{now + dt.timedelta(seconds=second):%Y-%m-%d-%H%M%S}.md" for second in range(120)]
    for path in taken:
        notes.write(notes.clone, path, "Earlier.\n")
    commit(notes.clone)
    git(notes.clone, "push", "-q")
    code, output = notes.call("add", "--text", "Later.", "--remote", VIITTA, tz="UTC")
    assert code == 0, output
    path = notes.note(output)
    assert path.removesuffix("-2.md") + ".md" in taken, path
    assert all((notes.clone / each).read_text() == "Earlier.\n" for each in taken), "no earlier note was touched"


@dataclass(frozen=True)
class Refusing(Repo):
    """A repo whose puts Board refuses with `codes` in turn, the last one ever after, recording each path put."""

    codes: tuple[str, ...] = (HEAD_MOVED,)
    puts: list[str] = field(default_factory=list[str])

    def pin(self) -> str | None:
        """A pin that never moves."""
        return None

    def read(self, path: str) -> Note:
        """No note at `path`."""
        del path
        return Note(None, None)

    def write(self, path: str, text: str, expect: str | None) -> str | None:
        """Refuse the put of `path`, as Board does after writing the file when HEAD moved."""
        del text, expect
        self.puts.append(path)
        code = self.codes[min(len(self.puts), len(self.codes)) - 1]
        raise PluginError("notes::board::refused", "refused", {"code": code})


def test_a_put_to_the_boards_own_tree_whose_head_moved_is_written_once_and_one_through_a_mount_is_retried() -> None:
    own = Refusing(Board("/board", "notes"), "/", None)
    assert append(own, "daily/d.md", "", "## 01:00\n\nOnce.\n") == (None, None), "written, not committed"
    assert own.puts == ["daily/d.md"]
    assert create(own, "inbox/i", "Once.\n") == ("inbox/i.md", None)
    mounted = Refusing(Board("/board", "notes"), "/repos/n", "/clone")
    assert append(mounted, "daily/d.md", "", "## 01:00\n\nAgain.\n") == (HEAD_MOVED, None)
    assert len(mounted.puts) == ATTEMPTS


def test_an_inbox_put_through_a_mount_whose_head_moved_retries_its_name_and_only_ten_taken_names_are_busy() -> None:
    landed = Refusing(Board("/board", "notes"), "/repos/n", "/clone", (HEAD_MOVED, STALE))
    assert create(landed, "inbox/i", "Once.\n") == ("inbox/i.md", None), "the stale put found its own file"
    assert landed.puts == ["inbox/i.md", "inbox/i.md"]
    moving = Refusing(Board("/board", "notes"), "/repos/n", "/clone")
    assert create(moving, "inbox/i", "Uncommitted.\n") == ("inbox/i.md", None), "written, not committed"
    assert moving.puts == ["inbox/i.md"] * ATTEMPTS
    taken = Refusing(Board("/board", "notes"), "/repos/n", "/clone", (STALE,))
    with pytest.raises(PluginError) as busy:
        create(taken, "inbox/i", "Never.\n")
    assert (busy.value.code, len(taken.puts)) == ("notes::note::busy", INBOX_NAMES)
    assert "call again" in busy.value.message


def test_the_boards_own_tree_that_does_not_commit_puts_reports_a_null_commit_and_one_that_does_its_commit(
    notes: Notes,
) -> None:
    git(notes.board, "rm", "-q", "repos/notes-personal.json")
    layout(notes.board)
    commit(notes.board)
    code, committed = notes.call("add", "--text", "Committed.")
    assert (code, committed["commit"]) == (0, git(notes.board, "rev-parse", "HEAD")), committed
    config = json.loads((notes.board / ".board" / "board.json").read_text()) | {"commit_on_put": False}
    notes.write(notes.board, ".board/board.json", json.dumps(config))
    commit(notes.board)
    code, output = notes.call("add", "--text", "Uncommitted.")
    assert code == 0, output
    assert (output["commit"], output["unpushed"]) == (None, "notes::repo::not_synced"), output
    assert (notes.board / output["path"].removeprefix("/")).read_text().endswith("Uncommitted.\n")


# The furthest ahead of UTC a time zone runs, in hours.
LATEST_OFFSET = 14


def zone_at_one_in_the_morning() -> str:
    """A fixed-offset time zone whose local time is now between 01:00 and 01:59."""
    offset = (1 - dt.datetime.now(dt.UTC).hour) % 24
    offset = offset - 24 if offset > LATEST_OFFSET else offset
    # Etc/GMT's sign is POSIX's, the opposite of ISO's: Etc/GMT-3 is three hours ahead of UTC.
    return "Etc/GMT" if offset == 0 else f"Etc/GMT{-offset:+d}"


def test_a_note_at_one_in_the_morning_lands_in_yesterdays_daily_note_when_the_day_starts_at_seven(
    notes: Notes,
) -> None:
    zone = zone_at_one_in_the_morning()
    local = dt.datetime.now(ZoneInfo(zone))
    assert local.hour == 1, (zone, local)
    yesterday = (local - dt.timedelta(days=1)).date().isoformat()
    notes.write(notes.config, "board/config.json", json.dumps({"day_start": "07:00"}))
    code, output = notes.call("add", "--text", "Late.", tz=zone)
    assert code == 0, output
    assert notes.note(output) == f"daily/{yesterday}.md"
    assert re.search(r"^## 0[12]:\d\d\n\nLate\.$", (notes.clone / notes.note(output)).read_text(), re.MULTILINE)
    code, read = notes.call("daily", tz=zone)
    assert (code, read["path"]) == (0, output["path"]), "daily reads the same day"
    notes.write(notes.config, "board/config.json", "{}")
    code, output = notes.call("add", "--text", "Midnight.", tz=zone)
    assert code == 0, output
    assert notes.note(output) in {f"daily/{date}.md" for date in dates(zone)}, "without day_start, today"


def test_a_day_start_that_is_not_hh_mm_is_refused_by_board_and_one_that_is_is_admitted(notes: Notes) -> None:
    notes.write(notes.config, "board/config.json", json.dumps({"day_start": "7am"}))
    code, refused = notes.call("add", "--text", "When?")
    assert (code, refused["code"]) == (1, "board::config::invalid")
    notes.write(notes.config, "board/config.json", json.dumps({"day_start": "07:00"}))
    code, output = notes.call("add", "--text", "When?")
    assert code == 0, output


def test_a_record_that_covers_nothing_is_admitted_and_one_naming_no_owner_is_refused() -> None:
    assert RepoRecord.model_validate({"$type": "notes/repo/v1", "covers": []}).covers == []
    with pytest.raises(ValidationError):
        RepoRecord.model_validate({"$type": "notes/repo/v1", "covers": ["home"]})
