"""The migration script on a committing board that has enabled the new schemas, through the real `board`."""

import json
from pathlib import Path
from typing import Any

import pytest
from conftest import Board

SESSION = "local_0a1b2c"
PR = "https://github.com/owner/app/pull/7"
# The schema plans shipped for the old decision, which an owner's board keeps while records use it.
OLD_SCHEMA = Path(__file__).parent / "fixtures" / "plans-decision-v1.schema.json"


@pytest.fixture(autouse=True)
def old_schema(board: Board) -> None:
    """The board keeps the old decision's schema, as `board plugin enable plans` does while records use it."""
    board.write(".board/types/plans/decision/v1.schema.json", json.loads(OLD_SCHEMA.read_text()))


def old_decision(**fields: Any) -> dict[str, Any]:  # noqa: ANN401 - the record's other fields, as JSON
    """A `plans/decision/v1` record."""
    return {
        "$type": "plans/decision/v1",
        "decision": "Ship it.",
        "why": "It is ready.",
        "session": "local_335a90f9",
        "decided": "2026-10-20T09:00:00Z",
        **fields,
    }


def stub() -> dict[str, Any]:
    """A `plans/decision/v1` stub as the runs mod writes it when a pull request is opened."""
    return old_decision(decision="Add a parser", why="", urls=[PR], review="wanted", written_by="runs/pr-create")


def old_eval(**fields: Any) -> dict[str, Any]:  # noqa: ANN401 - the record's other fields, as JSON
    """An `evals/eval/v1` record as it was written before evals derived their status."""
    return {"$type": "evals/eval/v1", "question": "Is gix faster?", "subjects": ["gix"], **fields}


def owners(board: Board) -> None:
    """Records like the owner's: decisions in two projects, some waiting on review, and evals of each status."""
    board.write(
        "projects/app/answers/a.json",
        {
            "$type": "plans/answer/v1",
            "question": "/projects/app/questions/x",
            "choice": "a",
            "words": "A.",
            "answered": "2026-10-19T09:00:00Z",
        },
    )
    board.write(
        "projects/app/decisions/kept.json",
        old_decision(alternatives=["Wait."], refs=["/projects/app/answers/a"], urls=[PR], review="none"),
    )
    board.write("projects/app/decisions/wanted.json", old_decision(review="wanted"))
    board.write("projects/app/decisions/stub.json", stub())
    board.write("projects/lib/decisions/plain.json", old_decision())
    board.write("projects/lib/decisions/filled.json", old_decision(written_by="runs/pr-create", urls=[PR]))
    board.write("evals/planned.json", old_eval(status="planned", decision="Pick a library.", mounts=["/repos/app"]))
    board.write("evals/early.json", old_eval(status="planned", runs=["/suites/s/runs/1"]))
    board.write("evals/stale.json", old_eval(status="running"))
    board.write("evals/done.json", old_eval(status="concluded", verdict="supported", runs=["/suites/s/runs/1"]))
    board.write("evals/dropped.json", old_eval(status="abandoned", conclusion="The library was withdrawn."))
    board.write("evals/silent.json", old_eval(status="abandoned"))
    board.write("evals/new.json", old_eval())


def test_a_dry_run_prints_every_rewrite_and_each_changed_status_and_writes_nothing(board: Board) -> None:
    owners(board)
    head = board.git("rev-parse", "HEAD")
    ran = board.migrate("--session", SESSION, "--dry-run")
    assert ran.returncode == 0, ran.stderr
    said = ran.stdout.splitlines()
    assert "rewrite /projects/app/decisions/wanted as decisions/decision/v1; it waits on the owner" in said
    assert "rewrite /projects/lib/decisions/plain as decisions/decision/v1" in said
    asks = [(line.split()[1].rsplit("/", 1)[0], line.split(" to ")[1]) for line in said if line.startswith("ask ")]
    assert asks == [("/projects/app/questions", "review 2 decisions"), ("/projects/lib/questions", "review 1 decision")]
    assert [line for line in said if line.startswith("eval ")] == [
        "eval /evals/early stored status planned and now derives running",
        "eval /evals/stale stored status running and now derives planned",
    ]
    assert "rewrite eval /evals/new" not in said
    assert said[-1] == "dry run: 13 records would be written; nothing was written"
    assert board.git("rev-parse", "HEAD") == head
    assert board.git("status", "--porcelain") == ""


def test_the_migration_keeps_every_field_but_those_it_drops_or_renames_and_checks_clean(board: Board) -> None:
    owners(board)
    before = board.checked()
    ran = board.migrate("--session", SESSION)
    assert ran.returncode == 0, ran.stderr
    assert board.git("status", "--porcelain") == ""
    assert board.record("projects/app/decisions/kept.json") == {
        "$type": "decisions/decision/v1",
        "decision": "Ship it.",
        "why": "It is ready.",
        "session": "local_335a90f9",
        "decided": "2026-10-20T09:00:00Z",
        "alternatives": ["Wait."],
        "basis": ["/projects/app/answers/a"],
        "urls": [PR],
    }
    assert board.record("projects/app/decisions/stub.json")["why"] == (
        f"This stub was written when the pull request {PR} was opened; no reason was recorded."
    )
    assert board.record("projects/lib/decisions/filled.json")["why"] == "It is ready."
    assert board.record("evals/planned.json") == old_eval(informs="Pick a library.", repos=["/repos/app"])
    assert board.record("evals/done.json") == old_eval(verdict="supported", runs=["/suites/s/runs/1"])
    assert board.record("evals/dropped.json")["abandoned"] == "The library was withdrawn."
    assert board.record("evals/silent.json")["abandoned"].endswith("no reason was recorded.")
    after = board.checked()
    # Only references the fixture leaves dangling (a run, a mount, an answer's question) degrade a record now.
    assert {path: code for path, code in after.items() if code not in (None, "board::ref::dangling")} == {}
    assert {path for path, code in before.items() if code == "board::record::invalid"} >= {"/evals/planned"}


def test_after_the_migration_open_lists_each_waiting_decision_under_one_question_per_project(board: Board) -> None:
    owners(board)
    assert board.migrate("--session", SESSION).returncode == 0
    listed = board.call("decisions.open", {})
    waiting = {item["path"]: item["questions"] for item in listed["decisions"]}
    assert sorted(waiting) == [
        "/projects/app/decisions/stub",
        "/projects/app/decisions/wanted",
        "/projects/lib/decisions/filled",
    ]
    [app] = {tuple(paths) for path, paths in waiting.items() if path.startswith("/projects/app/")}
    assert len(app) == 1
    assert app[0].startswith("/projects/app/questions/")
    assert waiting["/projects/lib/decisions/filled"][0].startswith("/projects/lib/questions/")
    questions = board.call("plans.open", {})["questions"]
    assert {question["session"] for question in questions} == {SESSION}
    texts = sorted(question["question"].split("?")[0] for question in questions)
    assert texts == ["Do you accept the 1 decision listed in `for`", "Do you accept the 2 decisions listed in `for`"]


def test_a_second_run_rewrites_only_the_stubs_written_since(board: Board) -> None:
    owners(board)
    assert board.migrate("--session", SESSION).returncode == 0
    board.write("projects/app/decisions/later.json", stub())
    ran = board.migrate("--session", SESSION)
    assert ran.returncode == 0, ran.stderr
    assert ran.stdout.splitlines()[-1].startswith("wrote 2 records; commit ")
    assert board.record("projects/app/decisions/later.json")["$type"] == "decisions/decision/v1"


def test_a_session_that_is_not_a_local_id_is_refused_and_one_that_is_is_taken(board: Board) -> None:
    refused = board.migrate("--session", "s", "--dry-run")
    assert (refused.returncode, refused.stdout) == (2, "")
    assert "--session 's' is not a local_ session id" in refused.stderr
    taken = board.migrate("--session", SESSION, "--dry-run")
    assert taken.returncode == 0, taken.stderr
