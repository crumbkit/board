"""`controls.evaluate` checks named parts of text against budget, pattern and judged rules."""

import json
import subprocess
from typing import Any

from conftest import Board, control

# An expression whose text differs from anything it matches, so a quote of a match never reveals it.
PRIVATE = r"acme-\d{3}"


def evaluate(
    board: Board, controls: list[str], parts: dict[str, str]
) -> tuple[subprocess.CompletedProcess[str], dict[str, Any]]:
    """Run `board --json call controls.evaluate` with the input on stdin, and read its one document."""
    stdin = json.dumps({"controls": controls, "parts": parts})
    ran = board.run("--json", "call", "controls.evaluate", stdin=stdin)
    document: dict[str, Any] = json.loads(ran.stdout)
    return ran, document


def output(document: dict[str, Any]) -> dict[str, Any]:
    """The capability's output in a `--json` report."""
    assert "data" in document, document
    result: dict[str, Any] = document["data"]["output"]
    return result


def failure(document: dict[str, Any]) -> dict[str, Any]:
    """The failure document a refused call prints."""
    assert "code" in document, document
    return document


def by_part(document: dict[str, Any]) -> dict[str, dict[str, Any]]:
    """The results of a call that checked one control, by part."""
    return {result["part"]: result for result in output(document)["results"]}


def test_a_part_over_a_budget_fails_and_one_within_it_passes(board: Board) -> None:
    board.write("controls/short.json", control("Short.", rule={"budget": "words", "max": 3}))
    ran, document = evaluate(board, ["/controls/short"], {"pr.title": "one two three", "pr.body": "one two three four"})
    assert ran.returncode == 0, ran.stderr
    assert output(document)["passed"] is False
    results = by_part(document)
    assert results["pr.title"] == {
        "control": "/controls/short",
        "part": "pr.title",
        "rule": "budget",
        "status": "passed",
        "findings": [],
        "used": 3,
        "max": 3,
    }
    assert results["pr.body"]["status"] == "failed"
    assert results["pr.body"]["findings"] == [{"text": "4 words, more than the 3 allowed"}]


def test_a_pattern_shown_match_only_quotes_the_match_and_its_line_and_never_the_expression(board: Board) -> None:
    board.write("private/names.txt", f"{PRIVATE}\n\nnever-matches\n")
    board.write(
        "controls/names.json", control("No names.", rule={"pattern": "/private/names.txt", "show": "match-only"})
    )
    ran, document = evaluate(board, ["/controls/names"], {"pr.body": "Fine.\nSee acme-123 here.", "ok": "Fine."})
    assert ran.returncode == 0, ran.stderr
    results = by_part(document)
    assert results["pr.body"]["findings"] == [{"text": "acme-123", "line": 2}]
    assert results["ok"]["status"] == "passed"
    assert PRIVATE not in ran.stdout + ran.stderr


def test_a_pattern_shown_as_pattern_names_the_expression(board: Board) -> None:
    board.write("private/names.txt", f"{PRIVATE}\n")
    board.write("controls/names.json", control("No names.", rule={"pattern": "/private/names.txt", "show": "pattern"}))
    _, document = evaluate(board, ["/controls/names"], {"pr.body": "See acme-123."})
    assert by_part(document)["pr.body"]["findings"] == [{"text": "acme-123", "line": 1, "pattern": PRIVATE}]


def test_an_invalid_expression_is_refused_by_its_line_without_its_text_and_a_valid_one_is_admitted(
    board: Board,
) -> None:
    board.write(
        "controls/names.json", control("No names.", rule={"pattern": "/private/names.txt", "show": "match-only"})
    )
    board.write("private/names.txt", f"{PRIVATE}\n{PRIVATE}[\n")
    ran, document = evaluate(board, ["/controls/names"], {"pr.body": "acme-123"})
    assert ran.returncode == 1
    assert failure(document)["code"] == "controls::pattern::invalid"
    assert failure(document)["fields"]["line"] == 2  # noqa: PLR2004 - the second line of the file
    assert PRIVATE not in ran.stdout + ran.stderr
    board.write("private/names.txt", f"{PRIVATE}\n{PRIVATE}\\[\n")
    ran, _ = evaluate(board, ["/controls/names"], {"pr.body": "acme-123"})
    assert ran.returncode == 0, ran.stderr


def test_the_parser_s_message_for_an_invalid_expression_never_reaches_the_output_under_match_only(board: Board) -> None:
    board.write(
        "controls/names.json", control("No names.", rule={"pattern": "/private/names.txt", "show": "match-only"})
    )
    for line in ["(?P=acmename)", "a{99999999999}"]:
        board.write("private/names.txt", f"{line}\n")
        ran, document = evaluate(board, ["/controls/names"], {"pr.body": "acme"})
        assert failure(document)["code"] == "controls::pattern::invalid", line
        assert "acmename" not in ran.stdout + ran.stderr
        assert "line 1 of /private/names.txt" in failure(document)["message"]
    board.write("private/names.txt", "(?P<acmename>acme)\n")
    ran, _ = evaluate(board, ["/controls/names"], {"pr.body": "acme"})
    assert ran.returncode == 0, ran.stderr


def test_findings_past_twenty_are_counted_and_left_out(board: Board) -> None:
    board.write("private/names.txt", "x\n")
    board.write("controls/names.json", control("No x.", rule={"pattern": "/private/names.txt", "show": "match-only"}))
    _, document = evaluate(board, ["/controls/names"], {"many": "x" * 25, "few": "x" * 20})
    results = by_part(document)
    assert (len(results["many"]["findings"]), results["many"]["omitted"]) == (20, 5)
    assert (len(results["few"]["findings"]), "omitted" in results["few"]) == (20, False)


def test_a_judged_rule_skips_a_part_that_failed_a_free_rule_and_is_refused_for_one_it_must_read(board: Board) -> None:
    board.write("rubrics/plain.md", "Write plain declarative sentences.\n")
    board.write("controls/plain.json", control("Plain prose.", rule={"judged": "/rubrics/plain.md"}))
    board.write("controls/short.json", control("Short.", rule={"budget": "words", "max": 1}))
    ran, document = evaluate(board, ["/controls/plain", "/controls/short"], {"pr.body": "Too long."})
    assert ran.returncode == 0, ran.stderr
    statuses = [(r["control"], r["status"]) for r in output(document)["results"]]
    assert statuses == [("/controls/plain", "skipped"), ("/controls/short", "failed")]
    ran, document = evaluate(board, ["/controls/plain", "/controls/short"], {"pr.body": "Short."})
    assert ran.returncode == 1
    assert failure(document)["code"] == "controls::judged::unavailable"
    assert failure(document)["fields"]["control"] == "/controls/plain"


def test_a_control_that_is_not_there_is_refused_and_one_that_is_is_admitted(board: Board) -> None:
    ran, document = evaluate(board, ["/controls/short"], {"pr.body": "Text."})
    assert ran.returncode == 1
    assert failure(document)["code"] == "controls::control::not_found"
    board.write("controls/short.json", control("Short.", rule={"budget": "words", "max": 3}))
    ran, _ = evaluate(board, ["/controls/short"], {"pr.body": "Text."})
    assert ran.returncode == 0, ran.stderr


def test_a_rule_file_that_is_missing_is_refused_and_one_that_is_there_is_admitted(board: Board) -> None:
    board.write("controls/names.json", control("No names.", rule={"pattern": "/private/names.txt", "show": "pattern"}))
    ran, document = evaluate(board, ["/controls/names"], {"pr.body": "Text."})
    assert ran.returncode == 1
    assert failure(document)["code"] == "controls::rule::unreadable"
    assert failure(document)["fields"]["file"] == "/private/names.txt"
    board.write("private/names.txt", "acme\n")
    ran, _ = evaluate(board, ["/controls/names"], {"pr.body": "Text."})
    assert ran.returncode == 0, ran.stderr


def test_a_control_without_a_rule_is_stated_and_never_fails(board: Board) -> None:
    board.write("controls/written.json", control("Be kind."))
    _, document = evaluate(board, ["/controls/written"], {"pr.body": "Text."})
    assert output(document) == {
        "passed": True,
        "results": [
            {"control": "/controls/written", "part": "pr.body", "rule": None, "status": "stated", "findings": []}
        ],
    }
