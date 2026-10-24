"""The `daily` capability and how a call chooses its vault, through the real `board`."""

import json

from conftest import Vaults, commit, dates, git, mount, vault


def test_the_daily_note_the_phone_pushed_is_read_after_a_sync(vaults: Vaults) -> None:
    vaults.phone_appends("daily/2026-01-05.md", "From the phone.\n")
    code, output = vaults.call("daily", "--date", "2026-01-05")
    assert code == 0, output
    head = git(vaults.remote, "rev-parse", "main")
    assert output == {
        "vault": "/vaults/personal",
        "path": "/vaults/personal/daily/2026-01-05.md",
        "exists": True,
        "private": False,
        "text": "From the phone.\n",
        "commit": head,
        "unsynced": None,
    }
    assert vaults.pin() == head


def test_the_day_read_by_default_is_today_in_the_callers_time_zone(vaults: Vaults) -> None:
    for zone in ("Pacific/Kiritimati", "Etc/GMT+12"):
        code, output = vaults.call("daily", tz=zone)
        assert code == 0, output
        assert output["path"] in {f"/vaults/personal/daily/{date}.md" for date in dates(zone)}, zone


def test_a_day_without_a_note_reads_as_none_and_creates_no_note(vaults: Vaults) -> None:
    before = git(vaults.vault, "rev-parse", "HEAD")
    code, output = vaults.call("daily", "--date", "2026-01-02")
    assert code == 0, output
    assert (output["exists"], output["text"]) == (False, None)
    assert not (vaults.vault / "daily" / "2026-01-02.md").exists()
    assert git(vaults.vault, "rev-parse", "HEAD") == before


def test_a_private_daily_note_is_not_returned_and_one_that_is_not_private_is(vaults: Vaults) -> None:
    vaults.phone_appends("daily/2026-01-02.md", "---\nprivate: true\n---\nSecret.\n")
    vaults.phone_appends("daily/2026-01-03.md", "---\nprivate: false\n---\nOpen.\n")
    code, hidden = vaults.call("daily", "--date", "2026-01-02")
    assert code == 0, hidden
    assert (hidden["exists"], hidden["private"], hidden["text"]) == (True, True, None)
    code, shown = vaults.call("daily", "--date", "2026-01-03")
    assert code == 0, shown
    assert (shown["exists"], shown["private"], shown["text"]) == (True, False, "---\nprivate: false\n---\nOpen.\n")


def test_a_date_that_is_not_a_day_is_refused_and_one_that_is_is_read(vaults: Vaults) -> None:
    code, refused = vaults.call("daily", "--date", "2026-02-30")
    assert (code, refused["code"]) == (1, "board::plugin::bad_input")
    code, read = vaults.call("daily", "--date", "2026-02-28")
    assert code == 0, read


def test_two_vaults_without_a_name_are_ambiguous_and_naming_one_reads_it(vaults: Vaults) -> None:
    work = vaults.board.parent / "work"
    vault(work, vaults.board.parent / "work.git")
    mount(vaults, "work", work)
    code, refused = vaults.call("daily")
    assert (code, refused["code"]) == (1, "notes::vault::ambiguous")
    code, read = vaults.call("daily", "--vault", "work")
    assert code == 0, read
    assert read["path"].startswith("/vaults/work/daily/")


def test_a_vault_that_is_not_mounted_is_refused_and_one_that_is_is_read(vaults: Vaults) -> None:
    code, refused = vaults.call("daily", "--vault", "work")
    assert (code, refused["code"]) == (1, "notes::vault::not_found")
    assert refused["fields"]["vaults"] == ["personal"]
    code, read = vaults.call("daily", "--vault", "personal")
    assert code == 0, read


def test_a_vault_record_that_mounts_the_board_itself_is_not_a_mount_of_a_vault(vaults: Vaults) -> None:
    pointer = {
        "$type": "board/mount/v1",
        "repo": str(vaults.board),
        "follow": "main",
        "commit": git(vaults.board, "rev-parse", "HEAD"),
    }
    vaults.write(vaults.board, "vaults/self.json", json.dumps(pointer))
    commit(vaults.board)
    code, refused = vaults.call("daily", "--vault", "self")
    assert (code, refused["code"]) == (1, "notes::vault::not_a_mount")
    assert refused["fields"]["vault"] == "/vaults/self"
    code, read = vaults.call("daily", "--vault", "personal")
    assert code == 0, read
