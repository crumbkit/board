"""The `daily` capability, through the real `board`."""

from conftest import Notes, dates, git


def test_the_daily_note_the_phone_pushed_is_read_after_a_sync(notes: Notes) -> None:
    notes.phone_appends("daily/2026-01-05.md", "From the phone.\n")
    code, output = notes.call("daily", "--date", "2026-01-05")
    assert code == 0, output
    head = git(notes.remote, "rev-parse", "main")
    assert output == {
        "repo": "/repos/notes-personal",
        "path": "/repos/notes-personal/daily/2026-01-05.md",
        "exists": True,
        "private": False,
        "text": "From the phone.\n",
        "commit": head,
        "unsynced": None,
    }
    assert notes.pin() == head


def test_the_day_read_by_default_is_today_in_the_callers_time_zone(notes: Notes) -> None:
    for zone in ("Pacific/Kiritimati", "Etc/GMT+12"):
        code, output = notes.call("daily", tz=zone)
        assert code == 0, output
        assert output["path"] in {f"/repos/notes-personal/daily/{date}.md" for date in dates(zone)}, zone


def test_a_day_without_a_note_reads_as_none_and_creates_no_note(notes: Notes) -> None:
    before = git(notes.clone, "rev-parse", "HEAD")
    code, output = notes.call("daily", "--date", "2026-01-02")
    assert code == 0, output
    assert (output["exists"], output["text"]) == (False, None)
    assert not (notes.clone / "daily" / "2026-01-02.md").exists()
    assert git(notes.clone, "rev-parse", "HEAD") == before


def test_a_private_daily_note_is_not_returned_and_one_that_is_not_private_is(notes: Notes) -> None:
    notes.phone_appends("daily/2026-01-02.md", "---\nprivate: true\n---\nSecret.\n")
    notes.phone_appends("daily/2026-01-03.md", "---\nprivate: false\n---\nOpen.\n")
    code, hidden = notes.call("daily", "--date", "2026-01-02")
    assert code == 0, hidden
    assert (hidden["exists"], hidden["private"], hidden["text"]) == (True, True, None)
    code, shown = notes.call("daily", "--date", "2026-01-03")
    assert code == 0, shown
    assert (shown["exists"], shown["private"], shown["text"]) == (True, False, "---\nprivate: false\n---\nOpen.\n")


def test_a_date_that_is_not_a_day_is_refused_and_one_that_is_is_read(notes: Notes) -> None:
    code, refused = notes.call("daily", "--date", "2026-02-30")
    assert (code, refused["code"]) == (1, "board::plugin::bad_input")
    code, read = notes.call("daily", "--date", "2026-02-28")
    assert code == 0, read
