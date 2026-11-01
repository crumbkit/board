"""The `artifacts/artifact/v1` and `artifacts/version/v1` schemas the plugin ships: what `board check` admits."""

import json

from conftest import Board, artifact, version

INVALID = "board::record::invalid"


def checked(board: Board) -> dict[str, str | None]:
    """Each record's code from `board check`, by its path; `None` when it is valid."""
    report = json.loads(board.run("--json", "check", "--all").stdout)
    return {
        entry["logical"]: entry.get("code") for entry in report["data"] if entry["logical"].startswith("/artifacts/")
    }


def test_an_artifact_is_a_claude_ai_link_an_account_and_a_time_with_its_zone(board: Board) -> None:
    board.write("artifacts/a.json", artifact())
    board.write("artifacts/code.json", artifact("https://claude.ai/code/artifact/0b8a7c6e-1f2d-4c3b-9a8e-7d6c5b4a3f21"))
    board.write("artifacts/elsewhere.json", artifact("https://example.com/artifact/AbCd"))
    board.write("artifacts/dotted.json", artifact("https://claude.ai/artifact/../AbCd"))
    board.write("artifacts/nobody.json", artifact(account=""))
    board.write("artifacts/no-zone.json", artifact(first_seen="2026-10-31T18:20:00"))
    assert checked(board) == {
        "/artifacts/a": None,
        "/artifacts/code": None,
        "/artifacts/elsewhere": INVALID,
        "/artifacts/dotted": INVALID,
        "/artifacts/nobody": INVALID,
        "/artifacts/no-zone": INVALID,
    }


def test_a_version_has_its_pages_hash_unless_it_says_it_has_no_copy(board: Board) -> None:
    copied = json.loads(version())
    board.write("artifacts/a/versions/copied.json", json.dumps(copied))
    uncopied = {key: value for key, value in copied.items() if key not in ("sha256", "source")}
    board.write("artifacts/a/versions/uncopied.json", json.dumps({**uncopied, "copy": "none"}))
    board.write("artifacts/a/versions/both.json", json.dumps({**copied, "copy": "none"}))
    board.write("artifacts/a/versions/neither.json", json.dumps(uncopied))
    board.write("artifacts/a/versions/short.json", version(sha256="abc"))
    assert checked(board) == {
        "/artifacts/a/versions/copied": None,
        "/artifacts/a/versions/uncopied": None,
        "/artifacts/a/versions/both": INVALID,
        "/artifacts/a/versions/neither": INVALID,
        "/artifacts/a/versions/short": INVALID,
    }


def test_a_version_names_its_session_and_time_and_may_leave_its_version_id_out(board: Board) -> None:
    keyed = {key: value for key, value in json.loads(version()).items() if key != "version"}
    board.write("artifacts/a/versions/0000000000000000.json", json.dumps(keyed))
    board.write("artifacts/a/versions/no-session.json", version(session=""))
    board.write("artifacts/a/versions/no-zone.json", version(at="2026-10-31T18:20:00"))
    board.write("artifacts/a/versions/slashed.json", version(version="1/2"))
    board.write("artifacts/a/versions/other.json", version(pinned=True))
    assert checked(board) == {
        "/artifacts/a/versions/0000000000000000": None,
        "/artifacts/a/versions/no-session": INVALID,
        "/artifacts/a/versions/no-zone": INVALID,
        "/artifacts/a/versions/slashed": INVALID,
        "/artifacts/a/versions/other": INVALID,
    }
