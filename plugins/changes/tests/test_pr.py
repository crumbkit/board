"""`changes.pr` over a real board: the body read from the change, its plan and its records, and the gh command."""

import shlex
from pathlib import Path
from typing import Any

from conftest import BASE, HEAD, PLAN, Board, record, report, setup

from board_changes.pr import quoted, run_lines, sections

PATH = "/changes/t"
PLAN_PATH = "/changes/t/plans/1.md"
RUN: dict[str, Any] = {
    "suite": "/suites/checks",
    "scope": "full",
    "dirty": False,
    "versions": {"app": HEAD},
    "results": [{"case": "a", "outcome": "pass"}, {"case": "b", "outcome": "pass"}],
}


def landing(board: Board, runs: tuple[str, ...] = ("/suites/checks/runs/1",), **fields: Any) -> None:  # noqa: ANN401 - the change's other fields, as JSON
    """A change with a plan, a check of each of `runs`, a passing run of the head and an approving review."""
    setup(board, **fields)
    board.write("changes/t/plans/1.md", PLAN)
    board.write("changes/t/plans/1.json", record("changes/plan/v1", text=PLAN_PATH, uses=[]))
    board.write("suites/checks/runs/1.json", record("runs/run/v1", **RUN))
    for number, run in enumerate(runs):
        board.write(f"changes/t/checks/{number}.json", record("changes/check/v1", run=run))
    brief = {"focus": "All.", "for": PATH, "step": "review", "range": f"{BASE}..{HEAD}"}
    board.write("reviews/1/brief.json", record("review/brief/v1", **brief))
    board.write("reviews/1/report.json", report(brief="/reviews/1/brief"))


def pr_text(board: Board) -> str:
    """The pr.md the plugin wrote for the change."""
    return (board.root / "changes/t/pr.md").read_text()


def test_pr_writes_the_body_beside_the_plan_and_returns_the_gh_command_that_would_use_it(board: Board) -> None:
    landing(board, issue="https://github.com/o/r/issues/2")
    output = board.call("pr", {"path": PATH})
    *words, file = shlex.split(output["command"])
    assert words == ["gh", "pr", "create", "--title", "Let a suite declare a timeout", "--body-file"]
    assert Path(file).resolve() == (board.root / "changes/t/pr.md").resolve()
    assert output == {"path": "/changes/t/pr.md", "command": output["command"], "pull_request": None, "unreadable": []}
    assert pr_text(board) == (
        "A suite stops.\n\n"
        "## Why\n\nSuites hang.\n\n"
        "## Out of scope\n\nRetries.\n\n"
        "## What the tests prove\n\n"
        "- `/suites/checks/runs/1`: `/suites/checks`, full run, results 2 pass\n\n"
        "## What the tests do not prove\n\n"
        "_The plan's What the tests do not prove section is empty._\n\n"
        "## Review\n\n"
        f"- `/reviews/1/report`: approve by `a reviewer` over {BASE}..{HEAD}, findings none\n\n"
        "## Evidence\n\n"
        "- `/changes/t`: the change\n"
        "- `/changes/t/plans/1.md`: the plan\n"
        "- `/suites/checks/runs/1`: runs/run/v1\n"
        "- `/reviews/1/report`: review/report/v1\n\n"
        "Closes https://github.com/o/r/issues/2\n"
    )


def test_the_plans_test_sections_lead_and_each_limit_of_a_run_is_stated(board: Board) -> None:
    older = RUN | {
        "scope": "targeted",
        "dirty": True,
        "versions": {"app": BASE},
        "results": [{"case": "a", "outcome": "pass"}, {"case": "b", "outcome": "skipped", "note": "needs macOS"}],
    }
    landing(board, runs=("/suites/checks/runs/0", "/suites/checks/runs/1"))
    board.write("suites/checks/runs/0.json", record("runs/run/v1", **older))
    sections = "\n## What the tests prove\n\nTimeouts stop a suite.\n\n## What the tests do not prove\n\nSignals.\n"
    board.write("changes/t/plans/1.md", PLAN + sections)
    board.call("pr", {"path": PATH})
    text = pr_text(board)
    assert "## What the tests prove\n\nTimeouts stop a suite.\n\n- `/suites/checks/runs/0`: " in text
    assert (
        "## What the tests do not prove\n\nSignals.\n\n"
        "- `/suites/checks/runs/0` ran only targeted cases.\n"
        "- `/suites/checks/runs/0` does not record a clean tree.\n"
        f"- `/suites/checks/runs/0` did not run on the head commit `{HEAD}`.\n"
        "- `/suites/checks/runs/0` case `b` is skipped: `needs macOS`\n\n"
    ) in text


def test_a_change_without_a_check_says_no_run_is_named_and_with_one_lists_it(board: Board) -> None:
    landing(board, runs=())
    board.call("pr", {"path": PATH})
    assert "- No check of this change names a test run." in pr_text(board)
    landing(board)
    board.call("pr", {"path": PATH})
    assert "No check of this change names a test run" not in pr_text(board)


def test_a_checked_run_that_reaches_nothing_or_is_degraded_is_unreadable_and_a_directory_is_cited(
    board: Board,
) -> None:
    landing(board, runs=("/suites/checks/runs/1", "/suites/gone", "/suites/checks/runs/2", "/suites/checks/runs"))
    board.write("suites/checks/runs/2.json", "{not json")
    output = board.call("pr", {"path": PATH})
    assert output["unreadable"] == ["/suites/gone", "/suites/checks/runs/2"]
    text = pr_text(board)
    assert "- `/suites/gone`: unreadable, board::path::not_found\n" in text
    assert "- `/suites/checks/runs/2`: unreadable, board::json::parse\n" in text
    assert "- `/suites/checks/runs`: directory\n" in text


def test_a_missing_plan_text_leaves_placeholders_that_name_the_empty_sections(board: Board) -> None:
    landing(board)
    (board.root / "changes/t/plans/1.md").unlink()
    board.call("pr", {"path": PATH})
    text = pr_text(board)
    assert text.startswith("_The plan's Outcome section is empty._\n\n## Why\n\n_The plan's Problem section is empty._")
    assert "- `/changes/t/plans/1.md`: the plan, missing\n" in text


def test_pr_replaces_the_pr_md_an_earlier_call_wrote(board: Board) -> None:
    landing(board)
    board.call("pr", {"path": PATH})
    board.write("changes/t/plans/1.md", PLAN.replace("A suite stops.", "A suite stops after its timeout."))
    board.call("pr", {"path": PATH})
    assert pr_text(board).startswith("A suite stops after its timeout.\n")


def test_a_path_that_holds_another_record_is_refused_and_the_same_path_holding_a_change_is_answered(
    board: Board,
) -> None:
    board.write("changes/t.json", record("plans/decision/v1", refs=[]))
    refused = board.refusal("pr", {"path": PATH})
    assert (refused["code"], refused["fields"]["path"]) == ("changes::change::not_found", PATH)
    assert not (board.root / "changes/t/pr.md").exists()
    landing(board)
    assert board.call("pr", {"path": PATH})["path"] == "/changes/t/pr.md"


def test_a_change_board_marks_degraded_is_refused_and_a_valid_one_is_answered(board: Board) -> None:
    landing(board, route="later")
    refused = board.refusal("pr", {"path": PATH})
    assert (refused["code"], refused["fields"]["code"]) == ("changes::change::degraded", "board::record::invalid")
    assert not (board.root / "changes/t/pr.md").exists()
    landing(board)
    assert board.call("pr", {"path": PATH})["path"] == "/changes/t/pr.md"


def test_the_path_is_taken_as_a_flag_relative_to_the_board(board: Board) -> None:
    landing(board)
    report_ = board.json("call", "changes.pr", "--path", "changes/t")
    assert report_["data"]["output"]["path"] == "/changes/t/pr.md"


def test_record_text_cannot_add_headings_list_items_mentions_or_closing_keywords_to_the_body(board: Board) -> None:
    run = RUN | {
        "summary": "Fixes #1\n## Owned by @someone",
        "results": [{"case": "b\n- c", "outcome": "fail", "note": "see `x`\n# loud"}],
    }
    landing(board, issue="https://github.com/o/r/issues/2 Fixes #3", title="Two\nlines")
    board.write("suites/checks/runs/1.json", record("runs/run/v1", **run))
    board.write("reviews/1/report.json", report(brief="/reviews/1/brief", by="@team\n# reviewer"))
    output = board.call("pr", {"path": PATH})
    assert shlex.split(output["command"])[4] == "Two lines"
    text = pr_text(board)
    assert "summary `Fixes #1 ## Owned by @someone`\n" in text
    assert "case `b - c` is fail: `see 'x' # loud`\n" in text
    assert "approve by `@team # reviewer` over " in text
    assert "Closes" not in text
    lines = text.splitlines()
    assert not [line for line in lines if line.startswith(("## Owned", "# loud", "# reviewer", "- c"))]


def test_a_well_formed_issue_is_closed_by_the_body(board: Board) -> None:
    landing(board, issue="https://github.com/o/r/issues/2")
    board.call("pr", {"path": PATH})
    assert pr_text(board).endswith("\n\nCloses https://github.com/o/r/issues/2\n")


def test_a_change_with_an_opened_record_gets_its_url_and_no_command(board: Board) -> None:
    url = "https://github.com/o/r/pull/1"
    landing(board)
    board.write("changes/t/opened.json", record("changes/opened/v1", pull_request=url))
    output = board.call("pr", {"path": PATH})
    assert (output["command"], output["pull_request"]) == (None, url)
    assert not (board.root / "changes/t/pr.md").exists()
    (board.root / "changes/t/opened.json").unlink()
    output = board.call("pr", {"path": PATH})
    assert (output["command"].startswith("gh pr create "), output["pull_request"]) == (True, None)
    assert pr_text(board).startswith("A suite stops.\n")


def test_a_report_for_another_change_is_not_shown_and_one_for_this_change_is(board: Board) -> None:
    landing(board)
    brief = {"focus": "All.", "for": "/changes/other", "step": "review", "range": f"{BASE}..{HEAD}"}
    board.write("reviews/2/brief.json", record("review/brief/v1", **brief))
    board.write("reviews/2/report.json", report(brief="/reviews/2/brief", by="another"))
    board.call("pr", {"path": PATH})
    text = pr_text(board)
    assert "`/reviews/1/report`: approve by `a reviewer`" in text
    assert "/reviews/2/report" not in text


def test_a_section_holds_its_subsections_and_a_hash_line_in_a_fence_is_not_a_heading() -> None:
    plan = "# T\n\n## Outcome\n\nStops.\n\n### Details\n\n```sh\n# comment\n```\n\n## Problem\n\nHangs.\n"
    found = sections(plan)
    assert found["outcome"] == "Stops.\n\n### Details\n\n```sh\n# comment\n```"
    assert found["details"] == "```sh\n# comment\n```"
    assert found["problem"] == "Hangs."
    assert "comment" not in found


def test_a_run_names_cases_up_to_the_cap_and_counts_the_rest() -> None:
    def run(count: int) -> dict[str, Any]:
        return RUN | {"results": [{"case": str(n), "outcome": "fail"} for n in range(count)]}

    _, limits = run_lines("/r", run(3), HEAD, cases=2)
    assert limits == ["`/r` case `0` is fail", "`/r` case `1` is fail", "and 1 more cases; see `/r`"]
    _, limits = run_lines("/r", run(2), HEAD, cases=2)
    assert limits == ["`/r` case `0` is fail", "`/r` case `1` is fail"]


def test_paths_and_the_suite_are_inline_code_and_empty_text_is_named() -> None:
    line, _ = run_lines("/runs/a`b", RUN | {"suite": "/suites/Fixes #1"}, HEAD)
    assert line.startswith("`/runs/a'b`: `/suites/Fixes #1`, full run, ")
    assert (quoted(""), quoted(" \n"), quoted("x")) == ("(empty)", "(empty)", "`x`")


def test_the_command_round_trips_a_title_with_shell_syntax_through_a_board_root_with_a_space(board: Board) -> None:
    assert " " in str(board.root)
    title = 'it\'s $(rm x) "q"'
    landing(board, title=title)
    *words, file = shlex.split(board.call("pr", {"path": PATH})["command"])
    assert words == ["gh", "pr", "create", "--title", title, "--body-file"]
    assert Path(file).resolve() == (board.root / "changes/t/pr.md").resolve()
