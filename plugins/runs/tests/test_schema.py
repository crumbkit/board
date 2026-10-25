"""The `runs/suite/v1` and `runs/run/v1` schemas the plugin ships: what `board check` admits and refuses."""

import json

from conftest import Board, run, suite

INVALID = "board::record::invalid"
FAILED = {"case": "a", "outcome": "fail", "note": "a timed out after 30 s"}


def checked(board: Board) -> dict[str, str | None]:
    """Each record's code from `board check`, by its path; `None` when it is valid."""
    report = json.loads(board.run("--json", "check", "--all").stdout)
    return {entry["logical"]: entry.get("code") for entry in report["data"] if entry["logical"].startswith("/suites/")}


def test_a_suite_lists_its_cases_inline_or_in_a_file_and_is_refused_with_both(board: Board) -> None:
    board.write("suites/cases.txt", "a\n")
    board.write("suites/inline.json", suite(cases=[{"id": "a"}]))
    board.write("suites/file.json", suite(cases_file="/suites/cases.txt"))
    board.write("suites/found.json", suite(command="cargo test"))
    board.write("suites/both.json", suite(cases=[{"id": "a"}], cases_file="/suites/cases.txt"))
    assert checked(board) == {
        "/suites/cases.txt": None,
        "/suites/inline": None,
        "/suites/file": None,
        "/suites/found": None,
        "/suites/both": INVALID,
    }


def test_a_suites_measure_needs_a_unit(board: Board) -> None:
    board.write("suites/with.json", suite(measures=[{"name": "median_ms", "unit": "ms"}]))
    board.write("suites/without.json", suite(measures=[{"name": "median_ms"}]))
    assert checked(board) == {"/suites/with": None, "/suites/without": INVALID}


def test_a_case_that_did_not_pass_needs_a_note_and_a_measured_case_does_not(board: Board) -> None:
    board.write("suites/a.json", suite())
    board.write("suites/a/runs/noted.json", run("/suites/a", results=[FAILED]))
    board.write("suites/a/runs/silent.json", run("/suites/a", results=[{"case": "a", "outcome": "fail"}]))
    board.write("suites/a/runs/skipped.json", run("/suites/a", results=[{"case": "a", "outcome": "skipped"}]))
    board.write("suites/a/runs/measured.json", run("/suites/a", results=[{"case": "a", "measures": {"ms": 4}}]))
    board.write("suites/a/runs/empty.json", run("/suites/a", results=[{"case": "a"}]))
    assert checked(board) == {
        "/suites/a": None,
        "/suites/a/runs/noted": None,
        "/suites/a/runs/silent": INVALID,
        "/suites/a/runs/skipped": INVALID,
        "/suites/a/runs/measured": None,
        "/suites/a/runs/empty": INVALID,
    }


def test_a_run_is_full_or_targeted_names_its_host_whole_and_gives_its_times_zone(board: Board) -> None:
    host = {"os": "macos", "arch": "aarch64", "logical_cpus": 12}
    board.write("suites/a.json", suite())
    board.write("suites/a/runs/full.json", run("/suites/a", host=host))
    board.write("suites/a/runs/targeted.json", run("/suites/a", scope="targeted"))
    board.write("suites/a/runs/partial.json", run("/suites/a", scope="partial"))
    board.write("suites/a/runs/no-cpus.json", run("/suites/a", host={"os": "macos", "arch": "aarch64"}))
    board.write("suites/a/runs/no-zone.json", run("/suites/a", "2026-10-01T09:00:00"))
    assert checked(board) == {
        "/suites/a": None,
        "/suites/a/runs/full": None,
        "/suites/a/runs/targeted": None,
        "/suites/a/runs/partial": INVALID,
        "/suites/a/runs/no-cpus": INVALID,
        "/suites/a/runs/no-zone": INVALID,
    }


def test_a_result_numbers_its_trial_from_one_and_its_evidence_must_exist(board: Board) -> None:
    board.write("suites/a.json", suite())
    board.write("suites/a/evidence/log.txt", "ok\n")
    first = {"case": "a", "outcome": "pass", "trial": 1, "evidence": ["/suites/a/evidence/log.txt"]}
    board.write("suites/a/runs/first.json", run("/suites/a", results=[first], setup="M4 Max", summary="All passed."))
    board.write("suites/a/runs/zeroth.json", run("/suites/a", results=[{**first, "trial": 0}]))
    board.write("suites/a/runs/lost.json", run("/suites/a", results=[{**first, "evidence": ["/suites/a/gone.txt"]}]))
    assert checked(board) == {
        "/suites/a": None,
        "/suites/a/evidence/log.txt": None,
        "/suites/a/runs/first": None,
        "/suites/a/runs/zeroth": INVALID,
        "/suites/a/runs/lost": "board::ref::dangling",
    }


def test_a_runs_suite_must_be_a_suite(board: Board) -> None:
    board.write("suites/a.json", suite())
    board.write("suites/notes.md", "# Notes\n")
    board.write("suites/a/runs/right.json", run("/suites/a"))
    board.write("suites/a/runs/wrong.json", run("/suites/notes.md"))
    board.write("suites/a/runs/gone.json", run("/suites/gone"))
    assert checked(board) == {
        "/suites/a": None,
        "/suites/notes.md": None,
        "/suites/a/runs/right": None,
        "/suites/a/runs/wrong": "board::ref::wrong_type",
        "/suites/a/runs/gone": "board::ref::dangling",
    }


def test_a_run_a_hook_wrote_names_the_plugin_and_hook_and_nothing_else(board: Board) -> None:
    board.write("suites/a.json", suite())
    board.write("suites/a/runs/hooked.json", run("/suites/a", written_by="runs/subagent-stop"))
    board.write("suites/a/runs/prose.json", run("/suites/a", written_by="the subagent hook"))
    board.write("suites/a/runs/empty.json", run("/suites/a", written_by=""))
    assert checked(board) == {
        "/suites/a": None,
        "/suites/a/runs/hooked": None,
        "/suites/a/runs/prose": INVALID,
        "/suites/a/runs/empty": INVALID,
    }
