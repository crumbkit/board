"""The `trend` capability, through the real `board`."""

import shutil

from conftest import Board, run, suite


def test_trend_returns_a_runs_measure_newest_first_from_full_clean_runs_only(board: Board) -> None:
    board.write("suites/a.json", suite())
    board.write("suites/a/runs/1.json", run("/suites/a", "2026-10-01T09:00:00Z", measures={"wall_s": 50}))
    board.write("suites/a/runs/2.json", run("/suites/a", "2026-10-02T09:00:00Z", measures={"wall_s": 42.5}))
    board.write("suites/a/runs/3.json", run("/suites/a", "2026-10-03T09:00:00Z", measures={"tests": 9}))
    board.write("suites/a/runs/4.json", run("/suites/a", "2026-10-04T09:00:00Z", measures={"wall_s": "fast"}))
    board.write("suites/a/runs/5.json", run("/suites/a", "2026-10-05T09:00:00Z", measures={"wall_s": True}))
    board.write("suites/a/runs/6.json", run("/suites/a", "2026-10-06T09:00:00Z", measures={"wall_s": 1}, dirty=True))
    output = board.call("trend", {"suite": "/suites/a", "measure": "wall_s"})
    assert output == {
        "points": [
            {"path": "/suites/a/runs/2", "started": "2026-10-02T09:00:00Z", "value": 42.5},
            {"path": "/suites/a/runs/1", "started": "2026-10-01T09:00:00Z", "value": 50.0},
        ],
        "total": 2,
        "truncated": False,
        "without_measure": 3,
        "not_counted": 1,
        "unreadable": 0,
    }


def test_trend_reads_one_cases_measure_when_given_a_case(board: Board) -> None:
    board.write("suites/bench.json", suite(measures=[{"name": "median_ms", "unit": "ms"}]))
    for day, get in ((1, 4.0), (2, 3.5)):
        results = [
            {"case": "get", "measures": {"median_ms": get}},
            {"case": "put", "measures": {"median_ms": 20}},
        ]
        board.write(
            f"suites/bench/runs/{day}.json",
            run("/suites/bench", f"2026-10-0{day}T09:00:00Z", results=results, measures={"median_ms": 99}),
        )
    output = board.call("trend", {"suite": "/suites/bench", "measure": "median_ms", "case": "get"})
    assert [point["value"] for point in output["points"]] == [3.5, 4.0]
    output = board.call("trend", {"suite": "/suites/bench", "measure": "median_ms"})
    assert [point["value"] for point in output["points"]] == [99.0, 99.0]
    output = board.call("trend", {"suite": "/suites/bench", "measure": "median_ms", "case": "list"})
    assert (output["points"], output["without_measure"]) == ([], 2)


def test_trend_reads_the_first_trial_of_a_case_that_ran_several(board: Board) -> None:
    board.write("suites/a.json", suite())
    results = [
        {"case": "cold", "trial": 2, "measures": {"wall_ms": 50}},
        {"case": "cold", "trial": 1, "measures": {"wall_ms": 40}},
    ]
    board.write("suites/a/runs/1.json", run("/suites/a", results=results))
    output = board.call("trend", {"suite": "/suites/a", "measure": "wall_ms", "case": "cold"})
    assert [point["value"] for point in output["points"]] == [40]


def test_trend_shows_at_most_its_limit_and_says_how_many_it_left_out(board: Board) -> None:
    board.write("suites/a.json", suite())
    for day in (1, 2, 3):
        board.write(f"suites/a/runs/{day}.json", run("/suites/a", f"2026-10-0{day}T09:00:00Z", measures={"s": day}))
    output = board.call("trend", {"suite": "/suites/a", "measure": "s", "limit": 2})
    assert [point["value"] for point in output["points"]] == [3.0, 2.0]
    assert (output["total"], output["truncated"]) == (3, True)


def test_trend_refuses_a_suite_that_does_not_exist_and_answers_one_that_does(board: Board) -> None:
    assert board.refusal("trend", {"suite": "/suites/a", "measure": "s"})["code"] == "runs::suite::not_found"
    board.write("suites/a.json", suite())
    assert board.call("trend", {"suite": "/suites/a", "measure": "s"})["total"] == 0


def test_trend_answers_the_same_from_a_cold_cache(board: Board) -> None:
    board.write("suites/a.json", suite())
    board.write("suites/a/runs/1.json", run("/suites/a", measures={"s": 1}))
    board.write("suites/a/runs/2.json", run("/suites/a", "2026-10-02T09:00:00Z", measures={"s": 2}))
    warm = board.call("trend", {"suite": "/suites/a", "measure": "s"})
    shutil.rmtree(board.cache)
    assert board.call("trend", {"suite": "/suites/a", "measure": "s"}) == warm
