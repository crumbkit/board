"""The `add` capability, through the real `board`, into a vault with a remote and a phone that pushes to it."""

import re
from concurrent.futures import ThreadPoolExecutor
from typing import Any

from conftest import Vaults, commit, dates, git

DAILY = r"daily/\d{4}-\d\d-\d\d\.md"


def remote_text(vaults: Vaults, path: str) -> str:
    """The file at `path` as the vault's remote holds it."""
    return git(vaults.remote, "show", f"main:{path}")


def sibling(path: str) -> str:
    """The `.agent.md` file beside the note at `path`."""
    return path.removesuffix(".md") + ".agent.md"


def test_a_note_lands_in_todays_daily_note_under_a_time_heading_and_reaches_the_remote(vaults: Vaults) -> None:
    vaults.write(vaults.vault, ".reflect/index.sqlite", "Reflect's own index")
    code, output = vaults.call("add", "--text", "Call the bank.")
    assert code == 0, output
    path = vaults.note(output)
    assert re.fullmatch(DAILY, path), path
    head = git(vaults.vault, "rev-parse", "HEAD")
    assert output == {
        "vault": "/vaults/personal",
        "path": f"/vaults/personal/{path}",
        "commit": head,
        "parked": None,
        "unpushed": None,
    }
    assert re.fullmatch(r"## \d\d:\d\d\n\nCall the bank\.\n", (vaults.vault / path).read_text())
    assert git(vaults.remote, "rev-parse", "main") == head
    assert vaults.pin() == head
    assert git(vaults.vault, "show", "--name-only", "--format=", head) == path, "only the note is committed"
    assert ".reflect" not in git(vaults.remote, "ls-tree", "-r", "--name-only", "main")


def test_the_daily_note_is_the_one_for_the_date_in_the_callers_time_zone(vaults: Vaults) -> None:
    # Kiritimati is 14 hours ahead of UTC and Etc/GMT+12 is 12 behind, so their dates always differ.
    for zone in ("Pacific/Kiritimati", "Etc/GMT+12"):
        code, output = vaults.call("add", "--text", "Here.", tz=zone)
        assert code == 0, output
        assert vaults.note(output) in {f"daily/{date}.md" for date in dates(zone)}, zone


def test_a_second_note_is_appended_as_a_new_section_after_what_the_phone_pushed(vaults: Vaults) -> None:
    _, first = vaults.call("add", "--text", "First.")
    path = vaults.note(first)
    vaults.phone_appends(path, "\nFrom the phone.\n")
    code, output = vaults.call("add", "--text", "Second.")
    assert code == 0, output
    assert vaults.note(output) == path
    sections = re.split(r"\n\n(?=## \d\d:\d\d\n)", remote_text(vaults, path))
    assert [section.split("\n\n", 1)[1] for section in sections] == ["First.\n\nFrom the phone.", "Second."]


def test_text_keeps_its_leading_indentation_and_text_of_only_whitespace_is_refused(vaults: Vaults) -> None:
    code, refused = vaults.call("add", "--text", " \n\t ")
    assert (code, refused["code"]) == (1, "board::plugin::bad_input")
    code, output = vaults.call("add", "--text", "    indented code\n\n")
    assert code == 0, output
    assert re.fullmatch(r"## \d\d:\d\d\n\n    indented code\n", (vaults.vault / vaults.note(output)).read_text())


def test_a_commit_the_desktop_app_made_and_did_not_push_is_kept_and_pushed_with_the_note(vaults: Vaults) -> None:
    vaults.write(vaults.vault, "notes/groceries.md", "# Groceries\n")
    desktop = commit(vaults.vault)
    code, output = vaults.call("add", "--text", "Buy milk.")
    assert code == 0, output
    assert git(vaults.remote, "merge-base", "--is-ancestor", desktop, "main") == ""
    assert remote_text(vaults, "notes/groceries.md") == "# Groceries"


def test_a_topic_note_is_started_with_its_title_at_reflects_slug_and_then_appended_to(vaults: Vaults) -> None:
    vaults.call("add", "--text", "Agenda.", "--topic", "Meeting Notes!")
    code, output = vaults.call("add", "--text", "Minutes.", "--topic", "Meeting Notes!")
    assert code == 0, output
    assert output["path"] == "/vaults/personal/notes/meeting-notes.md"
    text = remote_text(vaults, "notes/meeting-notes.md")
    assert re.fullmatch(r"# Meeting Notes!\n\n## \d\d:\d\d\n\nAgenda\.\n\n## \d\d:\d\d\n\nMinutes\.", text), text


def test_a_note_that_is_not_utf8_is_refused_and_left_as_it_was(vaults: Vaults) -> None:
    (vaults.phone / "notes" / "binary.md").write_bytes(b"\xff\xfe not text\n")
    commit(vaults.phone)
    git(vaults.phone, "push", "-q")
    code, refused = vaults.call("add", "--text", "More.", "--topic", "Binary")
    assert (code, refused["code"]) == (1, "notes::note::not_text")
    assert refused["fields"]["path"] == "/vaults/personal/notes/binary.md"
    assert (vaults.vault / "notes" / "binary.md").read_bytes() == b"\xff\xfe not text\n"
    code, output = vaults.call("add", "--text", "More.", "--topic", "Text")
    assert code == 0, output


def test_a_daily_note_with_an_edit_the_desktop_app_has_not_committed_parks_the_note_beside_it(vaults: Vaults) -> None:
    _, first = vaults.call("add", "--text", "First.")
    path = vaults.note(first)
    vaults.write(vaults.vault, path, (vaults.vault / path).read_text() + "\nTyping in Reflect")
    code, output = vaults.call("add", "--text", "Second.")
    assert code == 0, output
    assert (output["path"], output["parked"]) == (f"/vaults/personal/{sibling(path)}", "board::mount::stale")
    assert re.fullmatch(r"## \d\d:\d\d\n\nSecond\.", remote_text(vaults, sibling(path)))
    assert (vaults.vault / path).read_text().endswith("Typing in Reflect"), "the edit is left as it was"
    assert git(vaults.vault, "status", "--porcelain") == f"M {path}"


def test_a_daily_note_without_an_uncommitted_edit_is_not_parked(vaults: Vaults) -> None:
    _, first = vaults.call("add", "--text", "First.")
    path = vaults.note(first)
    vaults.write(vaults.vault, path, (vaults.vault / path).read_text() + "\nTyped in Reflect\n")
    commit(vaults.vault)
    code, output = vaults.call("add", "--text", "Second.")
    assert code == 0, output
    assert (output["path"], output["parked"]) == (f"/vaults/personal/{path}", None)


def test_a_note_and_its_sibling_both_holding_uncommitted_edits_are_busy_and_nothing_is_written(vaults: Vaults) -> None:
    _, first = vaults.call("add", "--text", "First.")
    path = vaults.note(first)
    vaults.write(vaults.vault, path, (vaults.vault / path).read_text() + "\nTyping in Reflect")
    vaults.call("add", "--text", "Parked.")
    vaults.write(vaults.vault, sibling(path), (vaults.vault / sibling(path)).read_text() + "\nMoving it")
    head = git(vaults.vault, "rev-parse", "HEAD")
    code, refused = vaults.call("add", "--text", "Third.")
    assert (code, refused["code"]) == (1, "notes::note::busy")
    assert refused["fields"]["path"] == f"/vaults/personal/{sibling(path)}"
    assert git(vaults.vault, "rev-parse", "HEAD") == head
    assert "Third." not in (vaults.vault / path).read_text() + (vaults.vault / sibling(path)).read_text()


def test_a_desktop_commit_beside_a_phone_push_to_another_note_syncs_and_is_not_parked(vaults: Vaults) -> None:
    _, first = vaults.call("add", "--text", "First.")
    path = vaults.note(first)
    vaults.write(vaults.vault, path, (vaults.vault / path).read_text() + "\nFrom the desktop.\n")
    commit(vaults.vault)
    vaults.phone_appends("notes/phone.md", "From the phone.\n")
    code, output = vaults.call("add", "--text", "From the agent.")
    assert code == 0, output
    assert (output["path"], output["parked"], output["unpushed"]) == (f"/vaults/personal/{path}", None, None)


def test_a_vault_that_cannot_sync_parks_the_note_and_leaves_no_merge_or_rebase_behind(vaults: Vaults) -> None:
    _, first = vaults.call("add", "--text", "First.")
    path = vaults.note(first)
    desktop = (vaults.vault / path).read_text() + "\nFrom the desktop.\n"
    vaults.write(vaults.vault, path, desktop)
    commit(vaults.vault)
    vaults.phone_appends(path, "\nFrom the phone.\n")
    code, output = vaults.call("add", "--text", "From the agent.")
    assert code == 0, output
    assert output["parked"] == "board::sync::conflict"
    assert output["unpushed"] == "board::sync::conflict"
    assert output["path"] == f"/vaults/personal/{sibling(path)}"
    assert output["commit"] == git(vaults.vault, "rev-parse", "HEAD") == vaults.pin()
    assert (vaults.vault / path).read_text() == desktop
    assert git(vaults.vault, "status", "--porcelain") == ""
    assert git(vaults.vault, "symbolic-ref", "--short", "HEAD") == "main"
    gitdir = vaults.vault / ".git"
    assert not any((gitdir / name).exists() for name in ("MERGE_HEAD", "REBASE_HEAD", "rebase-merge", "rebase-apply"))


def test_a_phone_push_over_an_uncommitted_edit_parks_the_note_and_leaves_the_edit(vaults: Vaults) -> None:
    _, first = vaults.call("add", "--text", "First.")
    path = vaults.note(first)
    vaults.phone_appends(path, "\nFrom the phone.\n")
    typing = (vaults.vault / path).read_text() + "\nTyping in Reflect"
    vaults.write(vaults.vault, path, typing)
    code, output = vaults.call("add", "--text", "From the agent.")
    assert code == 0, output
    assert (output["path"], output["parked"]) == (f"/vaults/personal/{sibling(path)}", "board::sync::edited")
    assert output["unpushed"] == "board::sync::edited"
    assert (vaults.vault / path).read_text() == typing
    assert re.fullmatch(r"## \d\d:\d\d\n\nFrom the agent\.\n", (vaults.vault / sibling(path)).read_text())


def test_a_push_refused_after_the_note_is_committed_is_reported_and_the_note_is_kept_once(vaults: Vaults) -> None:
    hook = vaults.remote / "hooks" / "pre-receive"
    hook.write_text("#!/bin/sh\nexit 1\n")
    hook.chmod(0o755)
    code, output = vaults.call("add", "--text", "Kept here.")
    assert code == 0, output
    assert (output["parked"], output["unpushed"]) == (None, "board::sync::gave_up")
    assert output["commit"] == git(vaults.vault, "rev-parse", "HEAD")
    assert (vaults.vault / vaults.note(output)).read_text().count("Kept here.") == 1


def test_notes_added_at_once_each_land_exactly_once(vaults: Vaults) -> None:
    texts = [f"Note {n}." for n in range(4)]

    def add(text: str) -> tuple[int, dict[str, Any]]:
        return vaults.call("add", "--text", text, "--topic", "Race")

    with ThreadPoolExecutor(len(texts)) as pool:
        results = list(pool.map(add, texts))
    assert [code for code, _ in results] == [0] * len(texts), results
    notes = [vaults.vault / "notes" / name for name in ("race.md", "race.agent.md")]
    written = "".join(note.read_text() for note in notes if note.exists())
    assert all(written.count(text) == 1 for text in texts), written
