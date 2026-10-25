"""The `add` capability, through the real `board`, into a notes repo with a remote and a phone that pushes to it."""

import re
from concurrent.futures import ThreadPoolExecutor
from typing import Any

from conftest import Notes, commit, dates, git

DAILY = r"daily/\d{4}-\d\d-\d\d\.md"


def remote_text(notes: Notes, path: str) -> str:
    """The file at `path` as the notes repo's remote holds it."""
    return git(notes.remote, "show", f"main:{path}")


def sibling(path: str) -> str:
    """The `.agent.md` file beside the note at `path`."""
    return path.removesuffix(".md") + ".agent.md"


def test_a_note_lands_in_todays_daily_note_under_a_time_heading_and_reaches_the_remote(notes: Notes) -> None:
    notes.write(notes.clone, ".reflect/index.sqlite", "Reflect's own index")
    code, output = notes.call("add", "--text", "Call the bank.")
    assert code == 0, output
    path = notes.note(output)
    assert re.fullmatch(DAILY, path), path
    head = git(notes.clone, "rev-parse", "HEAD")
    assert output == {
        "repo": "/repos/notes-personal",
        "path": f"/repos/notes-personal/{path}",
        "commit": head,
        "candidates": None,
        "parked": None,
        "unpushed": None,
    }
    assert re.fullmatch(r"## \d\d:\d\d\n\nCall the bank\.\n", (notes.clone / path).read_text())
    assert git(notes.remote, "rev-parse", "main") == head
    assert notes.pin() == head
    assert git(notes.clone, "show", "--name-only", "--format=", head) == path, "only the note is committed"
    assert ".reflect" not in git(notes.remote, "ls-tree", "-r", "--name-only", "main")


def test_the_daily_note_is_the_one_for_the_date_in_the_callers_time_zone(notes: Notes) -> None:
    # Kiritimati is 14 hours ahead of UTC and Etc/GMT+12 is 12 behind, so their dates always differ.
    for zone in ("Pacific/Kiritimati", "Etc/GMT+12"):
        code, output = notes.call("add", "--text", "Here.", tz=zone)
        assert code == 0, output
        assert notes.note(output) in {f"daily/{date}.md" for date in dates(zone)}, zone


def test_a_second_note_is_appended_as_a_new_section_after_what_the_phone_pushed(notes: Notes) -> None:
    _, first = notes.call("add", "--text", "First.")
    path = notes.note(first)
    notes.phone_appends(path, "\nFrom the phone.\n")
    code, output = notes.call("add", "--text", "Second.")
    assert code == 0, output
    assert notes.note(output) == path
    sections = re.split(r"\n\n(?=## \d\d:\d\d\n)", remote_text(notes, path))
    assert [section.split("\n\n", 1)[1] for section in sections] == ["First.\n\nFrom the phone.", "Second."]


def test_text_keeps_its_leading_indentation_and_text_of_only_whitespace_is_refused(notes: Notes) -> None:
    code, refused = notes.call("add", "--text", " \n\t ")
    assert (code, refused["code"]) == (1, "board::plugin::bad_input")
    code, output = notes.call("add", "--text", "    indented code\n\n")
    assert code == 0, output
    assert re.fullmatch(r"## \d\d:\d\d\n\n    indented code\n", (notes.clone / notes.note(output)).read_text())


def test_a_commit_the_desktop_app_made_and_did_not_push_is_kept_and_pushed_with_the_note(notes: Notes) -> None:
    notes.write(notes.clone, "notes/groceries.md", "# Groceries\n")
    desktop = commit(notes.clone)
    code, output = notes.call("add", "--text", "Buy milk.")
    assert code == 0, output
    assert git(notes.remote, "merge-base", "--is-ancestor", desktop, "main") == ""
    assert remote_text(notes, "notes/groceries.md") == "# Groceries"


def test_a_topic_note_is_started_with_its_title_at_reflects_slug_and_then_appended_to(notes: Notes) -> None:
    notes.call("add", "--text", "Agenda.", "--topic", "Meeting Notes!")
    code, output = notes.call("add", "--text", "Minutes.", "--topic", "Meeting Notes!")
    assert code == 0, output
    assert output["path"] == "/repos/notes-personal/notes/meeting-notes.md"
    text = remote_text(notes, "notes/meeting-notes.md")
    assert re.fullmatch(r"# Meeting Notes!\n\n## \d\d:\d\d\n\nAgenda\.\n\n## \d\d:\d\d\n\nMinutes\.", text), text


def test_a_note_that_is_not_utf8_is_refused_and_left_as_it_was(notes: Notes) -> None:
    (notes.phone / "notes" / "binary.md").write_bytes(b"\xff\xfe not text\n")
    commit(notes.phone)
    git(notes.phone, "push", "-q")
    code, refused = notes.call("add", "--text", "More.", "--topic", "Binary")
    assert (code, refused["code"]) == (1, "notes::note::not_text")
    assert refused["fields"]["path"] == "/repos/notes-personal/notes/binary.md"
    assert (notes.clone / "notes" / "binary.md").read_bytes() == b"\xff\xfe not text\n"
    code, output = notes.call("add", "--text", "More.", "--topic", "Text")
    assert code == 0, output


def test_a_daily_note_with_an_edit_the_desktop_app_has_not_committed_parks_the_note_beside_it(notes: Notes) -> None:
    _, first = notes.call("add", "--text", "First.")
    path = notes.note(first)
    notes.write(notes.clone, path, (notes.clone / path).read_text() + "\nTyping in Reflect")
    code, output = notes.call("add", "--text", "Second.")
    assert code == 0, output
    assert (output["path"], output["parked"]) == (f"/repos/notes-personal/{sibling(path)}", "board::mount::stale")
    assert re.fullmatch(r"## \d\d:\d\d\n\nSecond\.", remote_text(notes, sibling(path)))
    assert (notes.clone / path).read_text().endswith("Typing in Reflect"), "the edit is left as it was"
    assert git(notes.clone, "status", "--porcelain") == f"M {path}"


def test_a_daily_note_without_an_uncommitted_edit_is_not_parked(notes: Notes) -> None:
    _, first = notes.call("add", "--text", "First.")
    path = notes.note(first)
    notes.write(notes.clone, path, (notes.clone / path).read_text() + "\nTyped in Reflect\n")
    commit(notes.clone)
    code, output = notes.call("add", "--text", "Second.")
    assert code == 0, output
    assert (output["path"], output["parked"]) == (f"/repos/notes-personal/{path}", None)


def test_a_note_and_its_sibling_both_holding_uncommitted_edits_are_busy_and_nothing_is_written(notes: Notes) -> None:
    _, first = notes.call("add", "--text", "First.")
    path = notes.note(first)
    notes.write(notes.clone, path, (notes.clone / path).read_text() + "\nTyping in Reflect")
    notes.call("add", "--text", "Parked.")
    notes.write(notes.clone, sibling(path), (notes.clone / sibling(path)).read_text() + "\nMoving it")
    head = git(notes.clone, "rev-parse", "HEAD")
    code, refused = notes.call("add", "--text", "Third.")
    assert (code, refused["code"]) == (1, "notes::note::busy")
    assert refused["fields"]["path"] == f"/repos/notes-personal/{sibling(path)}"
    assert git(notes.clone, "rev-parse", "HEAD") == head
    assert "Third." not in (notes.clone / path).read_text() + (notes.clone / sibling(path)).read_text()


def test_a_desktop_commit_beside_a_phone_push_to_another_note_syncs_and_is_not_parked(notes: Notes) -> None:
    _, first = notes.call("add", "--text", "First.")
    path = notes.note(first)
    notes.write(notes.clone, path, (notes.clone / path).read_text() + "\nFrom the desktop.\n")
    commit(notes.clone)
    notes.phone_appends("notes/phone.md", "From the phone.\n")
    code, output = notes.call("add", "--text", "From the agent.")
    assert code == 0, output
    assert (output["path"], output["parked"], output["unpushed"]) == (f"/repos/notes-personal/{path}", None, None)


def test_a_notes_repo_that_cannot_sync_parks_the_note_and_leaves_no_merge_or_rebase_behind(notes: Notes) -> None:
    _, first = notes.call("add", "--text", "First.")
    path = notes.note(first)
    desktop = (notes.clone / path).read_text() + "\nFrom the desktop.\n"
    notes.write(notes.clone, path, desktop)
    commit(notes.clone)
    notes.phone_appends(path, "\nFrom the phone.\n")
    code, output = notes.call("add", "--text", "From the agent.")
    assert code == 0, output
    assert output["parked"] == "board::sync::conflict"
    assert output["unpushed"] == "board::sync::conflict"
    assert output["path"] == f"/repos/notes-personal/{sibling(path)}"
    assert output["commit"] == git(notes.clone, "rev-parse", "HEAD") == notes.pin()
    assert (notes.clone / path).read_text() == desktop
    assert git(notes.clone, "status", "--porcelain") == ""
    assert git(notes.clone, "symbolic-ref", "--short", "HEAD") == "main"
    gitdir = notes.clone / ".git"
    assert not any((gitdir / name).exists() for name in ("MERGE_HEAD", "REBASE_HEAD", "rebase-merge", "rebase-apply"))


def test_a_phone_push_over_an_uncommitted_edit_parks_the_note_and_leaves_the_edit(notes: Notes) -> None:
    _, first = notes.call("add", "--text", "First.")
    path = notes.note(first)
    notes.phone_appends(path, "\nFrom the phone.\n")
    typing = (notes.clone / path).read_text() + "\nTyping in Reflect"
    notes.write(notes.clone, path, typing)
    code, output = notes.call("add", "--text", "From the agent.")
    assert code == 0, output
    assert (output["path"], output["parked"]) == (f"/repos/notes-personal/{sibling(path)}", "board::sync::edited")
    assert output["unpushed"] == "board::sync::edited"
    assert (notes.clone / path).read_text() == typing
    assert re.fullmatch(r"## \d\d:\d\d\n\nFrom the agent\.\n", (notes.clone / sibling(path)).read_text())


def test_a_push_refused_after_the_note_is_committed_is_reported_and_the_note_is_kept_once(notes: Notes) -> None:
    hook = notes.remote / "hooks" / "pre-receive"
    hook.write_text("#!/bin/sh\nexit 1\n")
    hook.chmod(0o755)
    code, output = notes.call("add", "--text", "Kept here.")
    assert code == 0, output
    assert (output["parked"], output["unpushed"]) == (None, "board::sync::gave_up")
    assert output["commit"] == git(notes.clone, "rev-parse", "HEAD")
    assert (notes.clone / notes.note(output)).read_text().count("Kept here.") == 1


def test_notes_added_at_once_each_land_exactly_once(notes: Notes) -> None:
    texts = [f"Note {n}." for n in range(4)]

    def add(text: str) -> tuple[int, dict[str, Any]]:
        return notes.call("add", "--text", text, "--topic", "Race")

    with ThreadPoolExecutor(len(texts)) as pool:
        results = list(pool.map(add, texts))
    assert [code for code, _ in results] == [0] * len(texts), results
    files = [notes.clone / "notes" / name for name in ("race.md", "race.agent.md")]
    written = "".join(file.read_text() for file in files if file.exists())
    assert all(written.count(text) == 1 for text in texts), written
