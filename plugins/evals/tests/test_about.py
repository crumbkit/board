"""The `about` capability, through the real `board`."""

import json
import shutil
from pathlib import Path

from conftest import Board, experiment, mount, repository


def test_about_lists_the_evals_that_name_a_subject_whatever_their_status_with_their_question_and_verdict(
    board: Board,
) -> None:
    board.write("evals/planned.json", experiment("Planned?", ["gix"], decision="Pick a git library."))
    board.write(
        "evals/done.json",
        experiment("Done?", ["gix", "git2"], status="concluded", verdict="supported", conclusion="Yes."),
    )
    board.write("evals/other.json", experiment("Other?", ["serde"]))
    output = board.call("about", {"subject": "gix"})
    assert output["evals"] == [
        {
            "path": "/evals/done",
            "question": "Done?",
            "subjects": ["gix", "git2"],
            "status": "concluded",
            "verdict": "supported",
            "decision": None,
        },
        {
            "path": "/evals/planned",
            "question": "Planned?",
            "subjects": ["gix"],
            "status": "planned",
            "verdict": None,
            "decision": "Pick a git library.",
        },
    ]
    assert (output["total"], output["truncated"]) == (2, False)


def test_about_matches_a_subject_without_regard_to_case_and_misses_a_different_subject(board: Board) -> None:
    board.write("evals/a.json", experiment("A?", ["GitOxide"]))
    for asked in ("gitoxide", "GITOXIDE", "GitOxide"):
        assert board.paths("about", {"subject": asked}) == ["/evals/a"]
    assert board.paths("about", {"subject": "git"}) == []
    assert board.paths("about", {"subject": "gitoxide2"}) == []


def test_about_quotes_a_subject_so_it_matches_only_itself(board: Board) -> None:
    board.write("evals/a.json", experiment("A?", ["it's"]))
    board.write("evals/b.json", experiment("B?", ["other"]))
    assert board.paths("about", {"subject": "it's"}) == ["/evals/a"]
    assert board.paths("about", {"subject": "x' OR '1'='1"}) == []


def test_an_eval_that_is_removed_is_no_longer_listed(board: Board) -> None:
    board.write("evals/a.json", experiment("A?"))
    board.write("evals/b.json", experiment("B?"))
    assert board.paths("about", {"subject": "gix"}) == ["/evals/a", "/evals/b"]
    (board.root / "evals" / "a.json").unlink()
    assert board.paths("about", {"subject": "gix"}) == ["/evals/b"]


def test_a_record_of_another_type_or_one_the_schema_refuses_is_not_an_eval(board: Board) -> None:
    board.write("evals/ok.json", experiment("Ok?"))
    board.write("evals/odd-status.json", experiment("Odd?", status="someday"))
    # The plugin could read this one, which ignores fields it does not know; Board's verdict leaves it out.
    board.write("evals/colour.json", experiment("Colour?", colour="red"))
    board.write("notes/stray.json", '{"$type": "other/thing/v1", "subjects": ["gix"]}')
    assert board.paths("about", {"subject": "gix"}) == ["/evals/ok"]


def test_an_eval_the_boards_own_looser_schema_admits_but_the_plugin_cannot_read_is_not_listed(board: Board) -> None:
    schema = board.root / ".board" / "types" / "evals" / "eval" / "v1.schema.json"
    loosened = json.loads(schema.read_text())
    del loosened["if"], loosened["then"]
    schema.write_text(json.dumps(loosened))
    board.write("evals/bare.json", experiment("Bare?", status="concluded"))
    board.write("evals/ruled.json", experiment("Ruled?", status="concluded", verdict="refuted"))
    assert board.paths("about", {"subject": "gix"}) == ["/evals/ruled"]


def test_an_eval_whose_cases_file_reaches_nothing_is_still_listed(board: Board) -> None:
    board.write("evals/ok.json", experiment("Ok?", cases_file="/evals/ok/cases.jsonl"))
    board.write("evals/ok/cases.jsonl", "{}\n")
    board.write("evals/lost.json", experiment("Lost?", cases_file="/evals/lost/cases.jsonl"))
    assert board.paths("about", {"subject": "gix"}) == ["/evals/lost", "/evals/ok"]


def test_about_answers_the_same_from_a_cold_cache(board: Board) -> None:
    board.write("evals/a.json", experiment("A?", ["gix"], status="running"))
    board.write("evals/b.json", experiment("B?", ["gix"], status="concluded", verdict="refuted", conclusion="No."))
    warm = board.call("about", {"subject": "GIX"})
    shutil.rmtree(board.cache)
    assert board.call("about", {"subject": "GIX"}) == warm


def test_about_shows_at_most_its_limit_and_says_how_many_it_left_out(board: Board) -> None:
    for name in ("a", "b", "c"):
        board.write(f"evals/{name}.json", experiment(name.upper() + "?"))
    cut = board.call("about", {"subject": "gix", "limit": 2})
    assert [item["path"] for item in cut["evals"]] == ["/evals/a", "/evals/b"]
    assert (cut["total"], cut["truncated"]) == (3, True)
    whole = board.call("about", {"subject": "gix", "limit": 3})
    assert (len(whole["evals"]), whole["total"], whole["truncated"]) == (3, 3, False)


def test_a_limit_outside_one_to_a_hundred_or_an_empty_subject_is_refused_by_board_before_the_plugin_runs(
    board: Board,
) -> None:
    board.write("evals/a.json", experiment("A?"))
    for refused in (0, 101):
        error = board.json("call", "evals.about", stdin=json.dumps({"subject": "gix", "limit": refused}))
        assert error["code"] == "board::plugin::bad_input", error
    error = board.json("call", "evals.about", stdin=json.dumps({"subject": ""}))
    assert error["code"] == "board::plugin::bad_input", error
    for admitted in (1, 100):
        assert board.call("about", {"subject": "gix", "limit": admitted})["total"] == 1


def test_about_answers_across_two_mounted_repositories_and_the_boards_own_evals(board: Board, tmp_path: Path) -> None:
    first, first_commit = repository(
        tmp_path, "first", {"evals/pin-gix.json": experiment("Does gix pin?", ["gix"], decision="Pick a library.")}
    )
    second, second_commit = repository(
        tmp_path,
        "second",
        {
            "evals/clone-speed.json": experiment("Is gix fast?", ["gix", "git2"]),
            "evals/unrelated.json": experiment("Is serde fast?", ["serde"]),
        },
    )
    mount(board, "repos/first.json", first, first_commit)
    mount(board, "repos/second.json", second, second_commit)
    board.write("evals/local.json", experiment("Does gix build?", ["gix"]))
    everywhere = ["/evals/local", "/repos/first/evals/pin-gix", "/repos/second/evals/clone-speed"]
    assert board.paths("about", {"subject": "gix"}) == everywhere
    assert board.paths("about", {"subject": "serde"}) == ["/repos/second/evals/unrelated"]
    shutil.rmtree(board.cache)
    assert board.paths("about", {"subject": "gix"}) == everywhere


def test_a_mounted_eval_the_mounted_boards_schemas_refuse_is_not_listed_and_one_they_admit_is(
    board: Board, tmp_path: Path
) -> None:
    # The plugin could read this one; the mounted board's schema refuses its extra field.
    broken = experiment("Broken?", ["gix"], colour="red")
    mounted, commit = repository(
        tmp_path,
        "repo",
        {"evals/good.json": experiment("Good?", ["gix"]), "evals/broken.json": broken},
    )
    mount(board, "repos/repo.json", mounted, commit)
    assert board.paths("about", {"subject": "gix"}) == ["/repos/repo/evals/good"]
