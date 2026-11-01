"""The `ref` capability through the real `board`, with a stub `acli` or none, and its refusals in pairs."""

import json

import pytest
from conftest import Board

from board_jira.ref import title_from

ISSUE = {
    "$type": "jira/issue/v1",
    "site": "acme",
    "key": "ACME-123",
    "url": "https://acme.atlassian.net/browse/ACME-123",
}
VIEW = json.dumps({"key": "ACME-123", "fields": {"summary": "Fix the login redirect"}})


def test_ref_writes_the_issue_with_the_title_acli_reads(board: Board) -> None:
    board.acli(f"echo '{VIEW}'")
    code, output = board.ref("--key", "ACME-123", "--site", "acme")
    assert code == 0, output
    assert output == {
        "path": "/jira/acme/ACME-123",
        "site": "acme",
        "key": "ACME-123",
        "url": "https://acme.atlassian.net/browse/ACME-123",
        "title": "Fix the login redirect",
        "written": True,
        "title_note": None,
    }
    assert board.read("jira/acme/ACME-123.json") == ISSUE | {"title": "Fix the login redirect"}
    assert board.acli_calls() == ["jira workitem view ACME-123 --fields summary --json"]


def test_a_second_ref_writes_nothing_runs_no_acli_and_reports_the_record(board: Board) -> None:
    board.acli(f"echo '{VIEW}'")
    board.ref("--key", "ACME-123", "--site", "acme")
    before = (board.root / "jira/acme/ACME-123.json").read_text()
    code, output = board.ref("--key", "ACME-123", "--site", "acme")
    assert code == 0, output
    assert (output["written"], output["title"], output["title_note"]) == (False, "Fix the login redirect", None)
    assert (board.root / "jira/acme/ACME-123.json").read_text() == before
    assert len(board.acli_calls()) == 1


def test_without_acli_ref_writes_the_issue_without_a_title_and_says_why(board: Board) -> None:
    code, output = board.ref("--key", "ACME-123", "--site", "acme")
    assert code == 0, output
    assert (output["written"], output["title"]) == (True, None)
    assert "acli is not on PATH" in output["title_note"]
    assert board.read("jira/acme/ACME-123.json") == ISSUE


def test_when_acli_fails_ref_writes_the_issue_without_a_title_and_gives_acli_s_error(board: Board) -> None:
    board.acli("echo 'unauthorized: run acli jira auth login' >&2; exit 1")
    code, output = board.ref("--key", "ACME-123", "--site", "acme")
    assert code == 0, output
    assert output["title"] is None
    assert output["title_note"] == "acli exited 1: unauthorized: run acli jira auth login"
    assert board.read("jira/acme/ACME-123.json") == ISSUE


@pytest.mark.parametrize("key", ["acme-123", "ACME123", "ACME-0", "A-1", "ACME-12a", "1ACME-2"])
def test_a_key_that_is_not_project_hyphen_number_is_refused_and_its_twin_is_admitted(board: Board, key: str) -> None:
    code, refused = board.ref("--key", key, "--site", "acme")
    assert (code, refused["code"]) == (1, "jira::key::invalid"), refused
    assert refused["fields"]["key"] == key
    code, output = board.ref("--key", "AC_ME2-7", "--site", "acme")
    assert (code, output["path"]) == (0, "/jira/acme/AC_ME2-7"), output


@pytest.mark.parametrize("site", ["Acme", "acme.atlassian.net", "-acme", "acme/x"])
def test_a_site_that_is_not_an_atlassian_site_name_is_refused_and_its_twin_is_admitted(board: Board, site: str) -> None:
    code, refused = board.ref("--key", "ACME-1", f"--site={site}")
    assert (code, refused["code"]) == (1, "jira::site::invalid"), refused
    code, output = board.ref("--key", "ACME-1", "--site", "acme-eu2")
    assert (code, output["url"]) == (0, "https://acme-eu2.atlassian.net/browse/ACME-1"), output


def test_title_from_reads_the_summary_and_reports_output_without_one() -> None:
    assert title_from(VIEW, "acme") == ("Fix the login redirect", None)
    assert title_from("not json", "acme") == (None, "acli printed no JSON work item with a summary")
    assert title_from('{"fields": {"summary": ""}}', "acme") == (
        None,
        "acli printed no JSON work item with a summary",
    )


def test_title_from_drops_a_title_read_from_another_site_and_keeps_one_from_the_same_site() -> None:
    other = json.dumps({"self": "https://other.atlassian.net/rest/api/3/issue/1", "fields": {"summary": "T"}})
    same = json.dumps({"self": "https://acme.atlassian.net/rest/api/3/issue/1", "fields": {"summary": "T"}})
    assert title_from(other, "acme") == (
        None,
        "acli is signed in to another site (https://other.atlassian.net/rest/api/3/issue/1)",
    )
    assert title_from(same, "acme") == ("T", None)
