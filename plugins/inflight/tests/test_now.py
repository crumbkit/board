"""`inflight.now`: four changes stopped at different steps, their agents, a question and a pull request."""

from typing import Any

from conftest import Board, agent, change, checked, planned, record, stopped

STOP = "2026-11-01T09:00:00Z"
PR = "https://github.com/example/app/pull/7"


def stopped_board(board: Board) -> None:
    """Four changes, each stopped at a different step, with agents, a question, an eval and an artifact."""
    change(board, "a", session="s1")
    change(board, "b", session="s1")
    planned(board, "b")
    change(board, "c", session="s2")
    planned(board, "c")
    checked(board, "c")
    board.write("changes/c/opened.json", record("changes/opened/v1", pull_request=PR))
    change(board, "d", route="ask-first", session="s2")
    planned(board, "d")
    question = {"question": "Go ahead with d?", "session": "s2", "asked": "2026-11-01T08:10:00Z", "status": "open"}
    board.write("projects/p/questions/q1.json", record("plans/question/v1", **question, **{"for": ["/changes/d"]}))
    board.write("evals/e.json", record("evals/eval/v1", question="Is it fast?", subjects=["app"]))
    board.write("artifacts/x.json", record("artifacts/artifact/v1", url="https://claude.ai/artifact/x", account="o"))
    version = {"title": "Plan of c", "session": "s2", "at": "2026-11-01T08:20:00Z", "for": ["/changes/c"]}
    board.write("artifacts/x/versions/k.json", record("artifacts/version/v1", **version))
    agent(board, "early", "2026-11-01T08:30:00Z", works_on="/changes/b")
    agent(board, "late", "2026-11-01T09:30:00Z", works_on="/changes/b")
    stopped(board, agent(board, "done", "2026-11-01T08:45:00Z", works_on="/changes/c"))
    agent(board, "loose", "2026-11-01T08:50:00Z")
    board.write(
        "changes/gone.json",
        record(
            "changes/change/v2",
            title="Gone",
            repo="/repos/app",
            branch="gone",
            workflow="/workflows/personal",
            suite="/suites/checks",
            route="owner",
        ),
    )
    board.write("changes/gone/landed.json", record("changes/landed/v1", commit="a3f9c0d"))
    agent(board, "orphan", "2026-11-01T08:55:00Z", works_on="/changes/gone")


def by_path(output: dict[str, Any]) -> dict[str, dict[str, Any]]:
    return {entry["path"]: entry for entry in output["changes"]["changes"]}


def test_now_stopped_at_lists_each_change_with_its_next_step_and_actor(board: Board) -> None:
    stopped_board(board)
    output = board.now(**{"stopped-at": STOP})
    changes = by_path(output)
    assert {path: (entry["next"], entry["actor"]) for path, entry in changes.items()} == {
        "/changes/a": ("plan", "agent"),
        "/changes/b": ("build", "agent"),
        "/changes/c": ("review", "reviewer"),
        "/changes/d": ("approve", "owner"),
    }
    assert (changes["/changes/a"]["holder"], changes["/changes/a"]["worktree"]) == ("s1", "/work/a")
    assert [entry["refused"] for entry in changes.values()] == [None] * 4
    assert (output["changes"]["total"], output["changes"]["left_out"], output["changes"]["unreadable"]) == (4, 0, 0)


def test_now_groups_agents_with_no_run_under_their_change_and_stalls_those_started_before_the_stop(
    board: Board,
) -> None:
    stopped_board(board)
    output = board.now(**{"stopped-at": STOP})
    changes = by_path(output)
    assert [(each["path"], each["state"], each["prompt"]) for each in changes["/changes/b"]["agents"]] == [
        ("/agents/early", "stalled", "/agents/early.md"),
        ("/agents/late", "running", "/agents/late.md"),
    ]
    assert changes["/changes/c"]["agents"] == []
    assert [(each["path"], each["state"]) for each in output["unattached"]["agents"]] == [
        ("/agents/loose", "stalled"),
        ("/agents/orphan", "stalled"),
    ]


def test_without_stopped_at_every_agent_with_no_run_is_running(board: Board) -> None:
    stopped_board(board)
    output = board.now()
    states = [each["state"] for each in by_path(output)["/changes/b"]["agents"] + output["unattached"]["agents"]]
    assert states == ["running"] * 4


def test_now_shows_the_open_pull_request_with_the_command_that_checks_it_and_the_changes_artifacts(
    board: Board,
) -> None:
    stopped_board(board)
    changes = by_path(board.now())
    assert changes["/changes/c"]["pull_request"] == PR
    assert changes["/changes/c"]["pr_state"] == f"gh pr view {PR} --json state"
    assert (changes["/changes/a"]["pull_request"], changes["/changes/a"]["pr_state"]) == (None, None)
    assert changes["/changes/c"]["artifacts"] == [
        {
            "path": "/artifacts/x",
            "url": "https://claude.ai/artifact/x",
            "title": "Plan of c",
            "at": "2026-11-01T08:20:00Z",
        }
    ]
    assert changes["/changes/a"]["artifacts"] == []


def test_now_lists_the_waiting_question_once_and_the_open_eval(board: Board) -> None:
    stopped_board(board)
    output = board.now()
    assert [entry["path"] for entry in output["questions"]["entries"]] == ["/projects/p/questions/q1"]
    assert (output["questions"]["total"], output["questions"]["left_out"], output["questions"]["refused"]) == (
        1,
        0,
        None,
    )
    assert [entry["path"] for entry in output["evals"]["entries"]] == ["/evals/e"]
    assert all("questions" not in entry for entry in output["changes"]["changes"])


def test_holder_keeps_that_sessions_changes_their_agents_and_the_agents_it_started(board: Board) -> None:
    stopped_board(board)
    agent(board, "theirs", "2026-11-01T08:40:00Z", session="s2")
    output = board.now(holder="s1")
    assert list(by_path(output)) == ["/changes/a", "/changes/b"]
    assert [each["path"] for each in output["unattached"]["agents"]] == ["/agents/loose", "/agents/orphan"]
    assert [each["path"] for each in board.now(holder="s2")["unattached"]["agents"]] == ["/agents/theirs"]


def test_limit_bounds_each_part_and_counts_what_it_left_out(board: Board) -> None:
    stopped_board(board)
    output = board.now(limit=1)
    assert list(by_path(output)) == ["/changes/a"]
    assert (output["changes"]["total"], output["changes"]["left_out"]) == (4, 3)
    assert [each["path"] for each in output["unattached"]["agents"]] == ["/agents/loose"]
    assert (output["unattached"]["total"], output["unattached"]["left_out"]) == (2, 1)
    board.write("evals/f.json", record("evals/eval/v1", question="Is it small?", subjects=["app"]))
    assert (board.now(limit=1)["evals"]["total"], board.now(limit=1)["evals"]["left_out"]) == (2, 1)


def test_a_change_whose_steps_cannot_be_derived_says_why_and_the_rest_are_still_listed(board: Board) -> None:
    stopped_board(board)
    board.write(
        "changes/a.json",
        record(
            "changes/change/v2",
            title="A",
            repo="/repos/app",
            branch="a",
            workflow="/workflows/none",
            suite="/suites/checks",
            route="owner",
        ),
    )
    changes = by_path(board.now())
    assert (changes["/changes/a"]["next"], changes["/changes/a"]["refused"]["code"]) == (
        None,
        "changes::workflow::not_found",
    )
    assert (changes["/changes/b"]["next"], changes["/changes/b"]["refused"]) == ("build", None)


def test_an_agent_record_without_a_start_time_is_counted_as_unreadable_and_its_twin_is_listed(board: Board) -> None:
    agent(board, "fine", "2026-11-01T08:30:00Z")
    agent(board, "bad", "yesterday")
    output = board.now()
    assert [each["path"] for each in output["unattached"]["agents"]] == ["/agents/fine"]
    assert output["unattached"]["unreadable"] == 1


def test_a_stopped_at_without_a_time_zone_is_refused_and_one_with_a_zone_is_admitted(board: Board) -> None:
    refused = board.refusal("inflight.now", {"stopped-at": "2026-11-01T09:00:00"})
    assert refused["code"] == "board::plugin::bad_input"
    assert board.now(**{"stopped-at": STOP})["changes"]["changes"] == []
