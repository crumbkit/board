"""The `latest` capability, through the real `board`."""

import datetime
import shutil
from pathlib import Path

from conftest import Board, mount, repository, run, suite

FAILED = {"case": "slow", "outcome": "fail", "note": "slow took 31 s, over its 30 s limit"}
ERRED = {"case": "boot", "outcome": "error", "note": "boot could not bind its port"}
SKIPPED = {"case": "net", "outcome": "skipped", "note": "net needs a network"}


def test_latest_returns_the_newest_full_clean_run_and_counts_the_targeted_and_dirty_ones(board: Board) -> None:
    board.write("suites/a.json", suite())
    board.write("suites/a/runs/1.json", run("/suites/a", "2026-10-01T09:00:00Z"))
    board.write(
        "suites/a/runs/2.json",
        run(
            "/suites/a",
            "2026-10-02T09:00:00Z",
            finished="2026-10-02T09:05:00Z",
            results=[FAILED, SKIPPED, ERRED],
            measures={"tests": 40, "failures": 1},
        ),
    )
    board.write("suites/a/runs/3.json", run("/suites/a", "2026-10-03T09:00:00Z", scope="targeted"))
    board.write("suites/a/runs/4.json", run("/suites/a", "2026-10-04T09:00:00Z", dirty=True))
    output = board.call("latest", {"suite": "/suites/a"})
    age = output["run"].pop("age_s")
    assert output == {
        "run": {
            "path": "/suites/a/runs/2",
            "started": "2026-10-02T09:00:00Z",
            "finished": "2026-10-02T09:05:00Z",
            "outcome": "fail",
            "measures": {"tests": 40, "failures": 1},
            "failed": ["slow", "boot"],
        },
        "not_counted": 2,
        "unreadable": 0,
    }
    assert isinstance(output["run"]["measures"]["tests"], int)
    finished = datetime.datetime(2026, 10, 2, 9, 5, tzinfo=datetime.UTC)
    since = (datetime.datetime.now(datetime.UTC) - finished).total_seconds()
    assert since - 60 < age <= since


def test_a_run_whose_only_unpassed_cases_were_skipped_passes(board: Board) -> None:
    board.write("suites/a.json", suite())
    board.write("suites/a/runs/1.json", run("/suites/a", results=[SKIPPED]))
    assert board.call("latest", {"suite": "/suites/a"})["run"]["outcome"] == "pass"
    board.write("suites/a/runs/1.json", run("/suites/a", results=[SKIPPED, ERRED]))
    assert board.call("latest", {"suite": "/suites/a"})["run"]["outcome"] == "fail"


def test_latest_says_no_run_qualifies_when_every_run_is_targeted_or_dirty(board: Board) -> None:
    board.write("suites/a.json", suite())
    board.write("suites/a/runs/1.json", run("/suites/a", scope="targeted"))
    board.write("suites/a/runs/2.json", run("/suites/a", dirty=True))
    assert board.call("latest", {"suite": "/suites/a"}) == {"run": None, "not_counted": 2, "unreadable": 0}
    board.write("suites/a/runs/3.json", run("/suites/a"))
    assert board.call("latest", {"suite": "/suites/a"})["run"]["path"] == "/suites/a/runs/3"


def test_a_run_board_marks_degraded_is_counted_as_unreadable_and_a_valid_one_is_the_latest(board: Board) -> None:
    board.write("suites/a.json", suite())
    board.write("suites/a/runs/1.json", run("/suites/a", "2026-10-01T09:00:00Z"))
    board.write("suites/a/runs/2.json", run("/suites/a", "2026-10-02T09:00:00Z", scope="partial"))
    output = board.call("latest", {"suite": "/suites/a"})
    assert (output["run"]["path"], output["unreadable"]) == ("/suites/a/runs/1", 1)
    board.write("suites/a/runs/2.json", run("/suites/a", "2026-10-02T09:00:00Z"))
    output = board.call("latest", {"suite": "/suites/a"})
    assert (output["run"]["path"], output["unreadable"]) == ("/suites/a/runs/2", 0)


def test_runs_that_started_at_once_are_ordered_by_path_last_first(board: Board) -> None:
    board.write("suites/a.json", suite())
    board.write("suites/a/runs/20261001T090000Z-a.json", run("/suites/a"))
    board.write("suites/a/runs/20261001T090000Z-b.json", run("/suites/a"))
    assert board.call("latest", {"suite": "/suites/a"})["run"]["path"] == "/suites/a/runs/20261001T090000Z-b"


def test_latest_reads_only_the_suites_own_runs(board: Board) -> None:
    board.write("suites/a.json", suite())
    board.write("suites/ab.json", suite())
    board.write("suites/a/runs/1.json", run("/suites/a", "2026-10-01T09:00:00Z"))
    board.write("suites/ab/runs/1.json", run("/suites/ab", "2026-10-09T09:00:00Z"))
    assert board.call("latest", {"suite": "/suites/a"})["run"]["path"] == "/suites/a/runs/1"


def test_a_suite_that_does_not_exist_is_refused_and_one_that_does_is_answered(board: Board) -> None:
    board.write("suites/a/runs/1.json", run("/suites/a"))
    failure = board.refusal("latest", {"suite": "/suites/a"})
    assert failure["code"] == "runs::suite::not_found"
    board.write("suites/a.json", suite())
    assert board.call("latest", {"suite": "/suites/a"})["run"]["path"] == "/suites/a/runs/1"


def test_latest_reads_a_suite_in_a_mounted_board(board: Board, tmp_path: Path) -> None:
    records = {
        "suites/ci.json": suite(),
        "suites/ci/runs/1.json": run("/suites/ci", "2026-10-01T09:00:00Z"),
        "suites/ci/runs/2.json": run("/suites/ci", "2026-10-02T09:00:00Z"),
    }
    repo, commit = repository(tmp_path, "ci", records)
    mount(board, "boards/ci.json", repo, commit)
    output = board.call("latest", {"suite": "/boards/ci/suites/ci"})
    assert output["run"]["path"] == "/boards/ci/suites/ci/runs/2"


def test_latest_answers_the_same_from_a_cold_cache(board: Board) -> None:
    board.write("suites/a.json", suite())
    board.write("suites/a/runs/1.json", run("/suites/a", "2026-10-01T09:00:00Z", results=[FAILED]))
    board.write("suites/a/runs/2.json", run("/suites/a", "2026-10-02T09:00:00Z", dirty=True))
    warm = board.call("latest", {"suite": "/suites/a"})
    shutil.rmtree(board.cache)
    cold = board.call("latest", {"suite": "/suites/a"})
    warm["run"].pop("age_s")
    cold["run"].pop("age_s")
    assert cold == warm
