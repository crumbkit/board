"""`boundary.verify` finds a passing crossing, and `boundary.record` adds what the destination returned to one."""

import json
import subprocess
from typing import Any

from conftest import Board, boundary, private_names

PR = {"action": "github.pr-create", "to": "github:octo/board"}
URL = "https://github.com/octo/board/pull/7"


def checked(board: Board, body: str) -> str:
    """Check a pull request body against a boundary that refuses private names, and return its digest."""
    private_names(board)
    checks = [{"control": "/controls/names", "parts": ["*"]}]
    board.write("boundaries/octo.json", boundary(to="github:octo/*", actions=["github.*"], checks=checks))
    _, document = board.call("check", PR | {"parts": {"pr.body": body}})
    return str(document["fields"]["digest"] if "fields" in document else document["data"]["output"]["digest"])


def output(document: dict[str, Any]) -> dict[str, Any]:
    """The capability's output in a `--json` report."""
    assert "data" in document, document
    result: dict[str, Any] = document["data"]["output"]
    return result


def verify(board: Board, *flags: str) -> dict[str, Any]:
    """Run `board --json call boundary.verify` with `flags`, and read its output."""
    ran = board.run("--json", "call", "boundary.verify", *flags)
    assert ran.returncode == 0, ran.stderr
    return output(json.loads(ran.stdout))


def test_a_passing_crossing_verifies_by_digest_and_by_its_content(personal: Board) -> None:
    digest = checked(personal, "Fine.")
    expected = {"digest": digest, "crossing": f"/crossings/{digest}", "passed": True, "result": None}
    assert verify(personal, "--digest", digest) == expected
    _, document = personal.call("verify", PR | {"parts": {"pr.body": "Fine."}})
    assert output(document) == expected


def test_a_failed_crossing_and_a_missing_one_do_not_verify(personal: Board) -> None:
    digest = checked(personal, "acme-123")
    assert verify(personal, "--digest", digest)["passed"] is False
    _, document = personal.call("verify", PR | {"parts": {"pr.body": "Never checked."}})
    assert output(document)["crossing"] is None
    assert output(document)["passed"] is False


def test_verify_refuses_a_digest_given_with_content_and_takes_either_alone(personal: Board) -> None:
    digest = checked(personal, "Fine.")
    ran, document = personal.call("verify", PR | {"parts": {"pr.body": "Fine."}, "digest": digest})
    assert ran.returncode == 1
    assert document["code"] == "boundary::verify::bad_input"
    ran, document = personal.call("verify", {"to": PR["to"], "parts": {"pr.body": "Fine."}})
    assert document["code"] == "boundary::verify::bad_input"
    ran, _ = personal.call("verify", {"digest": digest})
    assert ran.returncode == 0, ran.stderr


def record(board: Board, digest: str) -> tuple[subprocess.CompletedProcess[str], dict[str, Any]]:
    """Run `board --json call boundary.record` for `digest` with URL as its result, and read its one document."""
    ran = board.run("--json", "call", "boundary.record", "--digest", digest, "--result", URL)
    document: dict[str, Any] = json.loads(ran.stdout)
    return ran, document


def test_record_adds_the_result_to_a_passing_crossing_and_verify_reports_it(personal: Board) -> None:
    digest = checked(personal, "Fine.")
    ran, document = record(personal, digest)
    assert ran.returncode == 0, ran.stderr
    assert output(document) == {"crossing": f"/crossings/{digest}", "result": URL}
    crossing = personal.record(f"/crossings/{digest}")
    assert crossing is not None
    assert crossing["result"] == URL
    assert crossing["passed"] is True
    assert verify(personal, "--digest", digest)["result"] == URL


def test_record_refuses_a_failed_crossing_and_one_never_checked(personal: Board) -> None:
    failed = checked(personal, "acme-123")
    ran, document = record(personal, failed)
    assert ran.returncode == 1
    assert document["code"] == "boundary::crossing::failed"
    assert "result" not in (personal.record(f"/crossings/{failed}") or {})
    ran, document = record(personal, "0" * 64)
    assert document["code"] == "boundary::crossing::not_found"
    passed = checked(personal, "Fine.")
    ran, _ = record(personal, passed)
    assert ran.returncode == 0, ran.stderr
