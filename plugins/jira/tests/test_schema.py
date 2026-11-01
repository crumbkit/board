"""The schemas the plugin ships: what `board check` admits and refuses in an issue and in the config."""

import json

from conftest import Board

ISSUE = {
    "$type": "jira/issue/v1",
    "site": "acme",
    "key": "ACME-123",
    "url": "https://acme.atlassian.net/browse/ACME-123",
}


def checked(board: Board) -> dict[str, str | None]:
    """Each record's code from `board check`, by its path; `None` when it is valid."""
    report = json.loads(board.run("--json", "check", "--all").stdout)
    return {entry["logical"]: entry.get("code") for entry in report["data"] if entry["logical"].startswith("/jira/")}


def test_an_issue_with_or_without_a_title_is_admitted_and_one_with_another_field_is_refused(board: Board) -> None:
    board.write("jira/acme/ACME-1.json", json.dumps(ISSUE))
    board.write("jira/acme/ACME-2.json", json.dumps(ISSUE | {"title": "Fix it"}))
    board.write("jira/acme/ACME-3.json", json.dumps(ISSUE | {"status": "Done"}))
    assert checked(board) == {
        "/jira/acme/ACME-1": None,
        "/jira/acme/ACME-2": None,
        "/jira/acme/ACME-3": "board::record::invalid",
    }


def test_an_issue_with_a_lowercase_key_or_a_url_off_atlassian_is_refused(board: Board) -> None:
    board.write("jira/acme/a.json", json.dumps(ISSUE | {"key": "acme-123"}))
    board.write("jira/acme/b.json", json.dumps(ISSUE | {"url": "https://example.com/browse/ACME-123"}))
    board.write("jira/acme/c.json", json.dumps(ISSUE | {"title": ""}))
    assert checked(board) == {
        "/jira/acme/a": "board::record::invalid",
        "/jira/acme/b": "board::record::invalid",
        "/jira/acme/c": "board::record::invalid",
    }


def test_the_config_holds_a_site_name_and_refuses_a_host(board: Board) -> None:
    board.write("jira/config.json", json.dumps({"$type": "jira/config/v1", "site": "acme"}))
    board.write("jira/other.json", json.dumps({"$type": "jira/config/v1", "site": "acme.atlassian.net"}))
    assert checked(board) == {"/jira/config": None, "/jira/other": "board::record::invalid"}
