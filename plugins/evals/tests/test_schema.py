"""The `evals/eval/v1` and `evals/run/v1` schemas the plugin ships: what `board check` admits and refuses."""

import json
from pathlib import Path
from typing import Any

from conftest import Board, experiment, mount, repository, trial

INVALID = "board::record::invalid"


def checked(board: Board, prefix: str = "/evals/") -> dict[str, str | None]:
    """Each record's code from `board check`, by its path; `None` when it is valid."""
    report = json.loads(board.run("--json", "check", "--all").stdout)
    return {entry["logical"]: entry.get("code") for entry in report["data"] if entry["logical"].startswith(prefix)}


def test_each_of_the_four_statuses_is_admitted_and_any_other_is_refused(board: Board) -> None:
    board.write("evals/planned.json", experiment("P?", status="planned"))
    board.write("evals/running.json", experiment("R?", status="running"))
    board.write("evals/abandoned.json", experiment("A?", status="abandoned"))
    board.write("evals/concluded.json", experiment("C?", status="concluded", verdict="supported"))
    board.write("evals/someday.json", experiment("S?", status="someday"))
    assert checked(board) == {
        "/evals/planned": None,
        "/evals/running": None,
        "/evals/abandoned": None,
        "/evals/concluded": None,
        "/evals/someday": INVALID,
    }


def test_a_concluded_eval_needs_a_verdict_and_an_unfinished_one_does_not(board: Board) -> None:
    board.write("evals/with.json", experiment("W?", status="concluded", verdict="refuted"))
    board.write("evals/without.json", experiment("W?", status="concluded"))
    board.write("evals/running.json", experiment("R?", status="running"))
    board.write("evals/odd.json", experiment("O?", status="concluded", verdict="maybe"))
    assert checked(board) == {
        "/evals/with": None,
        "/evals/without": INVALID,
        "/evals/running": None,
        "/evals/odd": INVALID,
    }


def test_an_eval_needs_a_question_subjects_a_method_and_a_status(board: Board) -> None:
    board.write("evals/whole.json", experiment("W?"))
    for missing in ("question", "subjects", "method", "status"):
        record: dict[str, Any] = json.loads(experiment("M?"))
        del record[missing]
        board.write(f"evals/no-{missing}.json", json.dumps(record))
    board.write("evals/no-subjects-listed.json", experiment("N?", subjects=[]))
    codes = checked(board)
    assert codes["/evals/whole"] is None
    assert {path: code for path, code in codes.items() if path != "/evals/whole"} == {
        "/evals/no-question": INVALID,
        "/evals/no-subjects": INVALID,
        "/evals/no-method": INVALID,
        "/evals/no-status": INVALID,
        "/evals/no-subjects-listed": INVALID,
    }


def test_an_eval_holds_cases_measures_and_a_decision_and_no_other_field(board: Board) -> None:
    board.write(
        "evals/full.json",
        experiment(
            "F?",
            cases=[{"id": "one", "input": {"x": 1}, "expected": "y"}],
            measures=[{"name": "latency", "unit": "ms", "description": "Wall time."}],
            decision="Pick one.",
        ),
    )
    board.write("evals/colour.json", experiment("C?", colour="red"))
    board.write("evals/case-without-id.json", experiment("C?", cases=[{"input": 1}]))
    assert checked(board) == {
        "/evals/full": None,
        "/evals/colour": INVALID,
        "/evals/case-without-id": INVALID,
    }


def test_a_run_needs_an_eval_who_ran_it_when_and_results(board: Board) -> None:
    board.write("evals/e.json", experiment("E?"))
    board.write("evals/e/runs/whole.json", trial("/evals/e"))
    for missing in ("eval", "by", "started", "results"):
        record: dict[str, Any] = json.loads(trial("/evals/e"))
        del record[missing]
        board.write(f"evals/e/runs/no-{missing}.json", json.dumps(record))
    board.write("evals/e/runs/bad-time.json", trial("/evals/e", started="yesterday"))
    codes = checked(board, "/evals/e/runs/")
    assert codes.pop("/evals/e/runs/whole") is None
    assert set(codes.values()) == {INVALID}, codes
    assert len(codes) == len(("eval", "by", "started", "results", "time"))


def test_a_result_needs_an_outcome_or_measures_and_either_is_enough(board: Board) -> None:
    board.write("evals/e.json", experiment("E?"))
    results = {
        "outcome": [{"case": "a", "outcome": "pass"}],
        "measures": [{"case": "a", "measures": {"latency": 3.5, "label": "fast", "ok": True}}],
        "both": [{"case": "a", "outcome": "fail", "measures": {"latency": 9}, "trial": 2}],
        "neither": [{"case": "a", "note": "forgot"}],
        "bad-outcome": [{"case": "a", "outcome": "meh"}],
        "bad-measure": [{"case": "a", "measures": {"latency": [1]}}],
    }
    for name, value in results.items():
        board.write(f"evals/e/runs/{name}.json", trial("/evals/e", results=value))
    assert checked(board, "/evals/e/runs/") == {
        "/evals/e/runs/outcome": None,
        "/evals/e/runs/measures": None,
        "/evals/e/runs/both": None,
        "/evals/e/runs/neither": INVALID,
        "/evals/e/runs/bad-outcome": INVALID,
        "/evals/e/runs/bad-measure": INVALID,
    }


def test_a_run_holds_versions_aggregate_measures_and_evidence(board: Board) -> None:
    board.write("evals/e.json", experiment("E?"))
    board.write("evals/e/evidence/log.txt", "ok\n")
    board.write(
        "evals/e/runs/full.json",
        trial(
            "/evals/e",
            versions={"model": "m-1", "gix": "0.70"},
            measures={"mean_ms": 12.5},
            setup="Laptop.",
            summary="Fine.",
            finished="2026-10-01T09:05:00Z",
            results=[{"case": "one", "outcome": "pass", "evidence": ["/evals/e/evidence/log.txt"], "trial": 1}],
        ),
    )
    board.write("evals/e/runs/bad-versions.json", trial("/evals/e", versions={"gix": 70}))
    board.write(
        "evals/e/runs/bad-trial.json", trial("/evals/e", results=[{"case": "a", "outcome": "pass", "trial": 0}])
    )
    assert checked(board, "/evals/e/runs/") == {
        "/evals/e/runs/full": None,
        "/evals/e/runs/bad-versions": INVALID,
        "/evals/e/runs/bad-trial": INVALID,
    }


def test_a_run_whose_eval_or_evidence_reaches_nothing_or_the_wrong_type_is_degraded(board: Board) -> None:
    board.write("evals/e.json", experiment("E?"))
    board.write("evals/e/runs/ok.json", trial("/evals/e"))
    board.write("evals/e/runs/lost.json", trial("/evals/missing"))
    board.write("evals/e/runs/wrong.json", trial("/evals/e/runs/ok"))
    board.write(
        "evals/e/runs/no-evidence.json",
        trial("/evals/e", results=[{"case": "a", "outcome": "pass", "evidence": ["/evals/e/evidence/gone.txt"]}]),
    )
    assert checked(board, "/evals/e/runs/") == {
        "/evals/e/runs/ok": None,
        "/evals/e/runs/lost": "board::ref::dangling",
        "/evals/e/runs/wrong": "board::ref::wrong_type",
        "/evals/e/runs/no-evidence": "board::ref::dangling",
    }


def test_an_eval_names_each_repository_by_its_mount_and_one_the_board_does_not_have_is_dangling(
    board: Board, tmp_path: Path
) -> None:
    crumbkit, commit = repository(tmp_path, "crumbkit", {})
    mount(board, "repos/crumbkit.json", crumbkit, commit)
    board.write("evals/mounted.json", experiment("M?", ["crumbkit"], repos=["/repos/crumbkit"]))
    board.write("evals/none.json", experiment("N?", ["crumbkit"]))
    board.write("evals/missing.json", experiment("L?", ["crumbkit"], repos=["/repos/crumbkit", "/repos/gone"]))
    board.write("evals/by-name.json", experiment("B?", ["crumbkit"], repos=["crumbkit/crumbkit"]))
    board.write("evals/not-a-list.json", experiment("S?", ["crumbkit"], repos="/repos/crumbkit"))
    assert checked(board) == {
        "/evals/mounted": None,
        "/evals/none": None,
        "/evals/missing": "board::ref::dangling",
        "/evals/by-name": "board::ref::malformed",
        "/evals/not-a-list": INVALID,
    }


def test_board_refs_on_a_repositorys_mount_lists_the_evals_that_name_it_and_no_others(
    board: Board, tmp_path: Path
) -> None:
    for name in ("crumbkit", "clap"):
        # Each holds a file of its own; two identical repositories would share one commit and be one object.
        root, commit = repository(tmp_path, name, {"README.md": name})
        mount(board, f"repos/{name}.json", root, commit)
    board.write("evals/a.json", experiment("A?", ["crumbkit"], repos=["/repos/crumbkit"]))
    board.write("evals/b.json", experiment("B?", ["crumbkit", "clap"], repos=["/repos/clap", "/repos/crumbkit"]))
    board.write("evals/c.json", experiment("C?", ["clap"], repos=["/repos/clap"]))
    board.write("evals/d.json", experiment("D?", ["crumbkit"]))

    def naming(path: str) -> list[tuple[str, list[str]]]:
        report = board.json("refs", path)
        return sorted((link["logical"], link["pointers"]) for link in report["data"][0]["to"])

    assert naming("/repos/crumbkit") == [("/evals/a", ["/repos/0"]), ("/evals/b", ["/repos/1"])]
    assert naming("/repos/clap") == [("/evals/b", ["/repos/0"]), ("/evals/c", ["/repos/0"])]
