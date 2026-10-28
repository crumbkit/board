"""`next`, `attach` and `advance` over a real board: the gate read from the references, and moving phases."""

from typing import Any

from conftest import HEAD, PLAN, Board, change, mount_repo, record, report, task

PATH = "/changes/t"


def test_next_reads_the_gate_from_the_plan_file_and_says_what_to_do(board: Board) -> None:
    board.write("changes/t.json", change(plan="/changes/t/plan.md", route="owner"))
    board.write("changes/t/plan.md", "# T\n\n## Problem\n\n## Outcome\n\n## Out of scope\n")
    output = board.call("next", {"path": PATH})
    assert (output["phase"], output["route"]) == ("plan", "owner")
    assert [check["status"] for check in output["gate"]] == ["missing", "pass"]
    assert output["instruction"].startswith("Interview the owner")
    board.write("changes/t/plan.md", PLAN)
    output = board.call("next", {"path": PATH})
    assert output["instruction"] == "The gate passes: run `board call changes.advance --path /changes/t`."


def test_advance_refuses_an_unmet_gate_naming_what_is_missing_and_moves_on_once_it_is_met(board: Board) -> None:
    board.write("changes/t.json", change(plan="/changes/t/plan.md", route="owner"))
    board.write("changes/t/plan.md", "# T\n")
    refused = board.refusal("advance", {"path": PATH})
    assert refused["code"] == "changes::gate::unmet"
    assert refused["fields"]["missing"] == ["plan"]
    assert board.read("changes/t.json")["phase"] == "plan"
    board.write("changes/t/plan.md", PLAN)
    assert board.call("advance", {"path": PATH}) == {"path": PATH, "from": "plan", "to": "build"}
    assert board.read("changes/t.json")["phase"] == "build"


def test_a_plan_with_no_route_blocks_the_plan_gate_and_setting_one_clears_it(board: Board) -> None:
    board.write("changes/t.json", change(plan="/changes/t/plan.md"))
    board.write("changes/t/plan.md", PLAN)
    assert board.refusal("advance", {"path": PATH})["fields"]["missing"] == ["route"]
    board.call("attach", {"path": PATH, "route": "unowned"})
    assert board.call("advance", {"path": PATH})["to"] == "build"


def test_a_degraded_referenced_record_satisfies_nothing_and_a_valid_one_does(board: Board) -> None:
    board.write("changes/t.json", change("review", head=HEAD, records=["/r/1"]))
    board.write("r/1.json", "{not json")
    assert board.refusal("advance", {"path": PATH})["fields"]["missing"] == ["review"]
    board.write("r/1.json", report())
    assert board.call("advance", {"path": PATH})["to"] == "land"


def test_a_referenced_record_degraded_only_by_a_dangling_reference_still_counts(board: Board) -> None:
    board.write("changes/t.json", change("review", head=HEAD, records=["/r/1"]))
    board.write("r/1.json", report(verdict="changes", brief="/briefs/none"))
    assert board.refusal("advance", {"path": PATH})["fields"]["missing"] == ["review"]
    board.write("r/1.json", report(brief="/briefs/none"))
    code = board.json("check", "/r/1")["data"][0].get("code")
    assert code == "board::ref::dangling"
    assert board.call("advance", {"path": PATH})["to"] == "land"


def test_a_referenced_record_deleted_after_it_was_attached_satisfies_nothing(board: Board) -> None:
    board.write("changes/t.json", change("review", head=HEAD))
    board.write("r/1.json", report())
    board.call("attach", {"path": PATH, "ref": "/r/1"})
    (board.root / "r/1.json").unlink()
    assert board.refusal("advance", {"path": PATH})["fields"]["missing"] == ["review"]
    board.write("r/1.json", report())
    assert board.call("advance", {"path": PATH})["to"] == "land"


def test_the_build_gate_finds_the_check_suite_in_the_repositorys_contributing_docs(board: Board) -> None:
    mount_repo(board, {"CONTRIBUTING.md": "Checks: `/suites/checks`.\n"})
    board.write("suites/checks.json", record("runs/suite/v1"))
    run: dict[str, Any] = {
        "suite": "/suites/checks",
        "scope": "full",
        "dirty": False,
        "versions": {"x": HEAD},
        "results": [],
    }
    board.write("suites/checks/runs/1.json", record("runs/run/v1", **run))
    board.write("changes/t.json", change("build", head=HEAD, records=["/suites/checks/runs/1"]))
    assert board.call("advance", {"path": PATH})["to"] == "review"


def test_the_build_gate_counts_a_run_of_a_suite_in_the_repositorys_own_board_by_where_the_run_lives(
    board: Board,
) -> None:
    run: dict[str, Any] = {"scope": "full", "dirty": False, "versions": {"x": HEAD}, "results": []}
    files = {
        "CONTRIBUTING.md": "Checks: `/suites/checks`.\n",
        ".board/types/runs/suite/v1.schema.json": "{}",
        ".board/types/runs/run/v1.schema.json": "{}",
        "suites/checks.json": record("runs/suite/v1"),
        "suites/checks/runs/1.json": record("runs/run/v1", **run),
        "suites/other.json": record("runs/suite/v1"),
        "suites/other/runs/1.json": record("runs/run/v1", suite="/repos/x/suites/checks", **run),
    }
    mount_repo(board, files)
    board.write("changes/t.json", change("build", head=HEAD, records=["/repos/x/suites/other/runs/1"]))
    assert board.refusal("advance", {"path": PATH})["fields"]["missing"] == ["check-run"]
    board.call("attach", {"path": PATH, "ref": "/repos/x/suites/checks/runs/1"})
    assert board.call("advance", {"path": PATH})["to"] == "review"


def test_next_on_a_done_change_has_no_gate_and_nothing_to_do(board: Board) -> None:
    board.write("changes/t.json", change("done"))
    output = board.call("next", {"path": PATH})
    assert (output["phase"], output["gate"], output["instruction"]) == ("done", [], "Nothing is left to do.")


def test_advance_runs_from_land_to_done_and_refuses_to_leave_done(board: Board) -> None:
    board.write("changes/t.json", change("land", pull_request="https://github.com/o/r/pull/1"))
    assert board.call("advance", {"path": PATH})["to"] == "done"
    refused = board.refusal("advance", {"path": PATH})
    assert (refused["code"], refused["fields"]["phase"]) == ("changes::phase::final", "done")
    assert "`board call changes.open`" in refused["message"]


def test_attach_refuses_a_reference_that_reaches_nothing_and_adds_one_that_does_once(board: Board) -> None:
    board.write("changes/t.json", change())
    board.write("tasks/a.md", task())
    refused = board.refusal("attach", {"path": PATH, "ref": "/tasks/b.md"})
    assert (refused["code"], refused["fields"]["ref"]) == ("changes::ref::dangling", "/tasks/b.md")
    assert board.call("attach", {"path": PATH, "ref": "/tasks/a.md"}) == {"path": PATH, "records": 1}
    assert board.call("attach", {"path": PATH, "ref": "/tasks/a.md"}) == {"path": PATH, "records": 1}
    assert board.read("changes/t.json")["records"] == ["/tasks/a.md"]


def test_attach_sets_the_route_head_issue_and_pull_request(board: Board) -> None:
    board.write("changes/t.json", change())
    url, issue = "https://github.com/o/r/pull/1", "https://github.com/o/r/issues/2"
    board.call("attach", {"path": PATH, "route": "owner", "head": HEAD, "pull_request": url})
    written = board.read("changes/t.json")
    assert (written["route"], written["head"], written["pull_request"]) == ("owner", HEAD, url)
    board.call("attach", {"path": PATH, "issue": issue})
    assert board.read("changes/t.json")["issue"] == issue


def test_attach_with_nothing_to_change_is_refused_by_its_input_schema_and_with_one_thing_is_admitted(
    board: Board,
) -> None:
    board.write("changes/t.json", change())
    refused = board.json("call", "changes.attach", "--path", PATH)
    assert refused["code"] == "board::plugin::bad_input"
    assert board.call("attach", {"path": PATH, "route": "owner"})["records"] == 0


def test_a_path_that_holds_no_change_is_refused_and_one_that_does_is_answered(board: Board) -> None:
    board.write("changes/t.json", change())
    board.write("tasks/a.md", task())
    for capability in ("next", "attach", "advance"):
        given = {"path": "/tasks/a.md", "route": "owner"} if capability == "attach" else {"path": "/tasks/a.md"}
        assert board.refusal(capability, given)["code"] == "changes::change::not_found", capability
    assert board.call("next", {"path": PATH})["phase"] == "plan"


def test_a_change_board_marks_degraded_is_refused_and_a_valid_one_is_answered(board: Board) -> None:
    board.write("changes/t.json", change(phase="later"))
    assert board.refusal("next", {"path": PATH})["code"] == "changes::change::degraded"
    board.write("changes/t.json", change())
    assert board.call("next", {"path": PATH})["phase"] == "plan"
