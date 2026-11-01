"""`controls.evaluate` checks named parts of text against budget, pattern and judged rules."""

import json
import shutil
import subprocess
from typing import Any

from conftest import Agent, Board, control

# An expression whose text differs from anything it matches, so a quote of a match never reveals it.
PRIVATE = r"acme-\d{3}"


def evaluate(
    board: Board, controls: list[str], parts: dict[str, str], agent: Agent | None = None
) -> tuple[subprocess.CompletedProcess[str], dict[str, Any]]:
    """Run `board --json call controls.evaluate` with the input on stdin, and read its one document."""
    extra = {} if agent is None else {"BOARD_CONTROLS_AGENT": json.dumps(agent.argv)}
    stdin = json.dumps({"controls": controls, "parts": parts})
    ran = board.run("--json", "call", "controls.evaluate", stdin=stdin, extra=extra)
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


def judged(board: Board) -> None:
    """A judged control and its rubric at /controls/plain and /rubrics/plain.md."""
    board.write("rubrics/plain.md", "Write plain declarative sentences.\n")
    board.write("controls/plain.json", control("Plain prose.", rule={"judged": "/rubrics/plain.md"}))


def test_a_judged_part_fails_with_the_model_s_findings_and_passes_without_them(board: Board, agent: Agent) -> None:
    judged(board)
    finding = {"line": 2, "text": "Measured, not guessed.", "rewrite": "Name who measured it."}
    agent.answers([finding])
    ran, document = evaluate(board, ["/controls/plain"], {"pr.body": "We measured it.\nMeasured, not guessed."}, agent)
    assert ran.returncode == 0, ran.stderr
    result = by_part(document)["pr.body"]
    assert (result["rule"], result["status"], result["findings"]) == ("judged", "failed", [finding])
    assert "Write plain declarative sentences." in agent.prompt()
    assert "    2| Measured, not guessed.\n" in agent.prompt()
    agent.answers([])
    _, document = evaluate(board, ["/controls/plain"], {"pr.body": "We measured it."}, agent)
    assert by_part(document)["pr.body"]["status"] == "passed"


def test_a_verdict_is_reused_for_the_same_text_rubric_and_agent_and_judged_again_when_one_changes(
    board: Board, agent: Agent
) -> None:
    judged(board)
    parts = {"pr.body": "We measured it."}
    evaluate(board, ["/controls/plain"], parts, agent)
    _, document = evaluate(board, ["/controls/plain"], parts, agent)
    assert (agent.calls(), by_part(document)["pr.body"]["status"]) == (1, "passed")
    evaluate(board, ["/controls/plain"], {"pr.body": "We measured it twice."}, agent)
    assert agent.calls() == 2  # noqa: PLR2004 - the changed text
    board.write("rubrics/plain.md", "Write short sentences.\n")
    evaluate(board, ["/controls/plain"], parts, agent)
    assert agent.calls() == 3  # noqa: PLR2004 - the changed rubric
    other = Agent(agent.directory)
    extra = {"BOARD_CONTROLS_AGENT": json.dumps([*other.argv, "--model", "another"])}
    stdin = json.dumps({"controls": ["/controls/plain"], "parts": parts})
    board.run("call", "controls.evaluate", stdin=stdin, extra=extra)
    assert agent.calls() == 4  # noqa: PLR2004 - the changed agent command


def test_a_deleted_cache_is_rebuilt_by_judging_again(board: Board, agent: Agent) -> None:
    judged(board)
    parts = {"pr.body": "We measured it."}
    evaluate(board, ["/controls/plain"], parts, agent)
    shutil.rmtree(board.cache / "board-controls")
    _, document = evaluate(board, ["/controls/plain"], parts, agent)
    assert (agent.calls(), by_part(document)["pr.body"]["status"]) == (2, "passed")
    _, document = evaluate(board, ["/controls/plain"], parts, agent)
    assert (agent.calls(), by_part(document)["pr.body"]["status"]) == (2, "passed")


def test_a_judged_rule_skips_a_part_that_failed_a_free_rule_whatever_order_the_controls_come_in(
    board: Board, agent: Agent
) -> None:
    judged(board)
    board.write("controls/short.json", control("Short.", rule={"budget": "words", "max": 3}))
    parts = {"pr.title": "Short title.", "pr.body": "A body that is far too long."}
    ran, document = evaluate(board, ["/controls/plain", "/controls/short"], parts, agent)
    assert ran.returncode == 0, ran.stderr
    statuses = [(r["control"], r["part"], r["status"]) for r in output(document)["results"]]
    assert statuses == [
        ("/controls/plain", "pr.body", "skipped"),
        ("/controls/plain", "pr.title", "passed"),
        ("/controls/short", "pr.body", "failed"),
        ("/controls/short", "pr.title", "passed"),
    ]
    assert agent.calls() == 1
    assert "Short title." in agent.prompt()


def test_free_rules_run_without_an_agent_and_a_judged_rule_with_a_part_to_read_needs_one(
    board: Board, agent: Agent
) -> None:
    judged(board)
    board.write("controls/short.json", control("Short.", rule={"budget": "words", "max": 1}))
    ran, document = evaluate(board, ["/controls/short", "/controls/plain"], {"pr.body": "Too long."})
    assert ran.returncode == 0, ran.stderr
    assert [result["status"] for result in output(document)["results"]] == ["failed", "skipped"]
    ran, document = evaluate(board, ["/controls/short", "/controls/plain"], {"pr.body": "Short."})
    assert ran.returncode == 1
    assert failure(document)["code"] == "controls::agent::not_set"
    ran, document = evaluate(board, ["/controls/short", "/controls/plain"], {"pr.body": "Short."}, agent)
    assert ran.returncode == 0, ran.stderr


def test_an_answer_without_findings_is_refused_and_one_with_them_is_admitted(board: Board, agent: Agent) -> None:
    judged(board)
    (agent.directory / "answer.txt").write_text('Looks fine.\n{"verdict": "approve"}\n')
    ran, document = evaluate(board, ["/controls/plain"], {"pr.body": "Text."}, agent)
    assert ran.returncode == 1
    assert failure(document)["code"] == "controls::agent::bad_output"
    agent.answers([{"text": "Text.", "rewrite": "Say more."}])
    ran, document = evaluate(board, ["/controls/plain"], {"pr.body": "Text."}, agent)
    assert ran.returncode == 0, ran.stderr
    assert by_part(document)["pr.body"]["findings"] == [{"text": "Text.", "rewrite": "Say more."}]


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
