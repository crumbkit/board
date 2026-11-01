"""`next`: a change's steps, each derived from which records exist, with a twin record set for each rule."""

from typing import Any

from conftest import BASE, HEAD, PATH, PLAN, Board, change, clone, mount, record, report, run, setup

SUITE_RUN = "/suites/checks/runs/1"


def steps(board: Board) -> dict[str, dict[str, Any]]:
    """The change's steps from `next`, by name."""
    return {step["name"]: step for step in board.call("next", {"path": PATH})["steps"]}


def checked(board: Board, name: str = "1", **fields: Any) -> None:  # noqa: ANN401 - the run's other fields
    """A run of the suite at `suites/checks/runs/<name>`, recorded as the change's newest check."""
    board.write(f"suites/checks/runs/{name}.json", run(**fields))
    board.call("check", {"path": PATH, "run": f"/suites/checks/runs/{name}"})


def reviewed(board: Board, review: str = "1", step: str = "review", **fields: Any) -> None:  # noqa: ANN401
    """A brief for the change's `step` and a report answering it, with the report's `fields`."""
    brief = {"focus": "Timeouts.", "for": PATH, "step": step, "repo": "/repos/app", "range": f"{BASE}..{HEAD}"}
    board.write(f"reviews/{review}/brief.json", record("review/brief/v1", **brief))
    board.write(f"reviews/{review}/report.json", report(brief=f"/reviews/{review}/brief", **fields))


def asked(board: Board, choice: str) -> None:
    """A question for the change and the owner's answer, `choice`."""
    board.write("projects/p/questions/q1.json", record("plans/question/v1", question="Go?", **{"for": [PATH]}))
    answer = record("plans/answer/v1", question="/projects/p/questions/q1", choice=choice)
    board.write("projects/p/answers/q1.json", answer)


def test_a_new_change_has_the_workflows_steps_none_done_and_the_plan_next(board: Board) -> None:
    setup(board)
    output = board.call("next", {"path": PATH})
    assert [(step["name"], step["actor"], step["done"]) for step in output["steps"]] == [
        ("plan", "agent", False),
        ("build", "agent", False),
        ("review", "reviewer", False),
        ("land", "owner", False),
    ]
    assert (output["head"], output["next"], output["actor"], output["ended"]) == (None, "plan", "agent", None)


def test_the_plan_step_is_done_by_a_plan_whose_uses_resolve_and_not_once_one_breaks(board: Board) -> None:
    setup(board)
    board.write("standards/a.md", "# A\n")
    written = board.call("plan", {"path": PATH, "text": PLAN, "uses": ["/standards/a.md"]}, stdin=True)
    assert (steps(board)["plan"]["done"], steps(board)["plan"]["record"]) == (True, written["path"])
    (board.root / "standards/a.md").unlink()
    assert steps(board)["plan"]["done"] is False


def test_the_check_step_is_done_by_a_full_clean_passing_run_of_the_suite(board: Board) -> None:
    setup(board)
    checked(board, results=[{"case": "a", "outcome": "fail", "note": "broke"}])
    assert steps(board)["build"]["done"] is False
    checked(board, "2")
    assert steps(board)["build"]["done"] is True


def test_the_check_step_reads_only_the_newest_check_and_refuses_a_targeted_or_dirty_run(board: Board) -> None:
    setup(board)
    checked(board)
    assert steps(board)["build"]["done"] is True
    checked(board, "2", scope="targeted")
    assert steps(board)["build"]["done"] is False
    checked(board, "3", dirty=True)
    assert steps(board)["build"]["done"] is False
    checked(board, "4")
    assert steps(board)["build"]["done"] is True


def test_the_check_step_refuses_a_run_of_another_suite_and_admits_one_of_the_changes(board: Board) -> None:
    setup(board)
    board.write("suites/other.json", record("runs/suite/v1", subjects=["app"], method="Run it."))
    board.write("suites/other/runs/1.json", run(suite="/suites/other"))
    board.call("check", {"path": PATH, "run": "/suites/other/runs/1"})
    assert steps(board)["build"]["done"] is False
    checked(board)
    assert steps(board)["build"]["done"] is True


def test_the_review_step_is_done_by_an_approving_report_for_this_step_at_the_head(board: Board) -> None:
    setup(board)
    checked(board)
    reviewed(board, "1", step="security-review")
    assert steps(board)["review"]["done"] is False
    reviewed(board, "2", verdict="changes")
    assert steps(board)["review"]["done"] is False
    reviewed(board, "3", findings=[{"severity": "high", "text": "Leaks a key."}])
    assert steps(board)["review"]["done"] is False
    reviewed(board, "4", range=f"{BASE}..{BASE}")
    assert steps(board)["review"]["done"] is False
    reviewed(board, "5", findings=[{"severity": "low", "text": "A nit."}])
    assert (steps(board)["review"]["done"], steps(board)["review"]["record"]) == (True, "/reviews/5/report")


def test_a_new_head_undoes_the_review_step_until_a_review_covers_it(board: Board) -> None:
    setup(board)
    board.call("plan", {"path": PATH, "text": PLAN, "uses": []}, stdin=True)
    checked(board)
    reviewed(board)
    assert steps(board)["review"]["done"] is True
    newer = "b" * 40
    checked(board, "2", versions={"app": newer})
    output = board.call("next", {"path": PATH})
    assert (output["head"], output["next"]) == (newer, "review")
    reviewed(board, "2", range=f"{BASE}..{newer}")
    assert board.call("next", {"path": PATH})["next"] == "land"


def test_the_land_step_is_done_by_the_landed_record_which_ends_the_change(board: Board) -> None:
    setup(board)
    assert steps(board)["land"]["done"] is False
    board.call("landed", {"path": PATH, "commit": HEAD})
    output = board.call("next", {"path": PATH})
    assert (output["steps"][-1]["done"], output["steps"][-1]["record"]) == (True, f"{PATH}/landed")
    assert (output["next"], output["actor"], output["ended"]) == (None, None, "landed")


def test_a_dropped_change_has_ended_and_has_no_next_step(board: Board) -> None:
    setup(board)
    board.call("drop", {"path": PATH, "why": "Not needed."})
    output = board.call("next", {"path": PATH})
    assert (output["next"], output["ended"]) == (None, "dropped")


def test_the_ask_first_route_adds_an_approve_step_after_the_plan_done_by_an_approving_answer(board: Board) -> None:
    setup(board, route="ask-first")
    approve = steps(board)["approve"]
    assert (approve["actor"], approve["done_by"], approve["added_by"], approve["done"]) == (
        "owner",
        "answer",
        "route",
        False,
    )
    assert list(steps(board))[:2] == ["plan", "approve"]
    asked(board, "later")
    assert steps(board)["approve"]["done"] is False
    asked(board, "approve")
    assert (steps(board)["approve"]["done"], steps(board)["approve"]["record"]) == (True, "/projects/p/answers/q1")


def test_the_unread_route_adds_the_approve_step_to_the_work_workflow_but_not_to_the_personal_one(board: Board) -> None:
    setup(board, route="unread")
    assert "approve" not in steps(board)
    board.write("changes/t.json", change(route="unread", workflow="/workflows/work"))
    assert list(steps(board)) == ["plan", "approve", "build", "review", "land"]
    board.write("changes/t.json", change(route="owner", workflow="/workflows/work"))
    assert steps(board)["approve"]["added_by"] is None


def secrets_control(board: Board) -> None:
    """A control on /repos/app's secrets that adds a security review."""
    control = {
        "requirement": "Secrets changes get a security review.",
        "purpose": "A mistake there leaks a credential.",
        "repos": ["/repos/app"],
        "paths": ["src/secrets/*"],
        "adds_step": "security-review",
    }
    board.write("controls/secrets.json", record("controls/control/v1", **control))


def test_a_controls_step_appears_only_when_the_files_record_at_the_head_includes_a_match(board: Board) -> None:
    setup(board)
    secrets_control(board)
    checked(board)
    board.call("files", {"path": PATH, "head": HEAD, "files": ["README.md"]}, stdin=True)
    assert "security-review" not in steps(board)
    board.call("files", {"path": PATH, "head": HEAD, "files": ["README.md", "src/secrets/key.rs"]}, stdin=True)
    added = steps(board)["security-review"]
    assert (added["actor"], added["done_by"], added["added_by"], added["if_touched"]) == (
        "reviewer",
        "review",
        "/controls/secrets",
        False,
    )
    assert list(steps(board)) == ["plan", "build", "review", "security-review", "land"]
    reviewed(board, "1", step="security-review")
    assert steps(board)["security-review"]["done"] is True


def test_without_a_files_record_at_the_head_a_mounted_repositorys_controls_list_their_step_if_touched(
    board: Board,
) -> None:
    setup(board)
    (board.root / "repos/app.json").unlink()
    mount(board, clone(board, {"src/secrets/key.rs": "k\n"}))
    secrets_control(board)
    assert steps(board)["security-review"]["if_touched"] is True
    board.call("files", {"path": PATH, "head": HEAD, "files": ["src/secrets/key.rs"]}, stdin=True)
    checked(board)
    assert steps(board)["security-review"]["if_touched"] is False


def test_a_change_whose_workflow_is_gone_is_refused_and_one_whose_workflow_is_there_is_answered(
    board: Board,
) -> None:
    setup(board, workflow="/workflows/gone")
    assert board.refusal("next", {"path": PATH})["code"] == "changes::workflow::not_found"
    board.write("changes/t.json", change())
    assert board.call("next", {"path": PATH})["next"] == "plan"


def test_seed_writes_each_missing_workflow_and_leaves_one_already_there(board: Board) -> None:
    board.write(
        "workflows/work.json", record("changes/workflow/v1", steps=[{"name": "x", "actor": "agent", "done_by": "plan"}])
    )
    assert board.call("seed", {}) == {"written": ["/workflows/personal"]}
    assert board.read("workflows/work.json")["steps"] == [{"name": "x", "actor": "agent", "done_by": "plan"}]
    assert [step["name"] for step in board.read("workflows/personal.json")["steps"]] == [
        "plan",
        "build",
        "review",
        "land",
    ]
    assert board.call("seed", {}) == {"written": []}


def test_a_change_goes_from_start_to_landed_with_next_reporting_each_step(board: Board) -> None:
    board.call("seed", {})
    board.write("suites/checks.json", record("runs/suite/v1", subjects=["app"], method="Run it."))
    cloned = clone(board)
    given = {"title": "T", "session": "s", "suite": "/suites/checks", "workflow": "/workflows/personal"}
    board.call("start", given, cwd=cloned.path)

    def following() -> tuple[str | None, str | None]:
        output = board.call("next", {"path": "/changes/t"})
        return output["next"], output["actor"]

    assert following() == ("plan", "agent")
    board.call("plan", {"path": "/changes/t", "text": PLAN, "uses": []}, stdin=True)
    assert following() == ("build", "agent")
    board.write("suites/checks/runs/1.json", run())
    board.call("check", {"path": "/changes/t", "run": SUITE_RUN})
    assert following() == ("review", "reviewer")
    brief = {"focus": "All.", "for": "/changes/t", "step": "review", "repo": "/repos/app", "range": f"{BASE}..{HEAD}"}
    board.write("reviews/1/brief.json", record("review/brief/v1", **brief))
    board.write("reviews/1/report.json", report(brief="/reviews/1/brief"))
    assert following() == ("land", "owner")
    board.call("landed", {"path": "/changes/t", "commit": HEAD})
    assert following() == (None, None)
