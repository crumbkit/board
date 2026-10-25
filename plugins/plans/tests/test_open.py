"""`plans.open`: the questions still waiting for the owner."""

import json
import subprocess
from pathlib import Path

from conftest import Board, question


def paths(output: dict[str, object]) -> list[object]:
    """The paths of the questions an output lists, in order."""
    listed = output["questions"]
    assert isinstance(listed, list)
    return [entry["path"] for entry in listed]  # pyright: ignore[reportUnknownVariableType]


def test_open_lists_open_questions_oldest_first_and_leaves_out_answered_and_withdrawn_ones(board: Board) -> None:
    board.write("projects/a/questions/late.json", question(asked="2026-10-03T09:00:00Z"))
    board.write("projects/a/questions/early.json", question(asked="2026-10-01T09:00:00Z"))
    board.write("projects/a/questions/done.json", question("answered"))
    board.write("projects/a/questions/gone.json", question("withdrawn"))
    output = board.call("open", {})
    assert paths(output) == ["/projects/a/questions/early", "/projects/a/questions/late"]
    assert (output["total"], output["truncated"], output["unreadable"]) == (2, False, 0)


def test_open_shows_what_the_owner_needs_to_answer(board: Board) -> None:
    options = [{"id": "a", "text": "Go."}, {"id": "b", "text": "Stay."}]
    board.write("projects/a/questions/q.json", question(options=options, recommendation="a", blocks="step 2"))
    [listed] = board.call("open", {})["questions"]
    assert listed == {
        "path": "/projects/a/questions/q",
        "question": "Which way?",
        "asked": "2026-10-01T09:00:00Z",
        "session": "local_0a1b",
        "options": options,
        "recommendation": "a",
        "blocks": "step 2",
    }


def test_open_lists_only_questions_under_the_path_it_is_given(board: Board) -> None:
    board.write("projects/a/questions/q.json", question())
    board.write("projects/ab/questions/q.json", question())
    assert paths(board.call("open", {"path": "/projects/a"})) == ["/projects/a/questions/q"]
    assert paths(board.call("open", {"path": "/projects/a/"})) == ["/projects/a/questions/q"]


def test_open_returns_at_most_the_limit_and_says_how_many_it_left_out(board: Board) -> None:
    for day in (1, 2, 3):
        board.write(f"projects/a/questions/{day}.json", question(asked=f"2026-10-0{day}T09:00:00Z"))
    output = board.call("open", {"limit": 2})
    assert paths(output) == ["/projects/a/questions/1", "/projects/a/questions/2"]
    assert (output["total"], output["truncated"]) == (3, True)


def test_a_question_whose_only_fault_is_a_dangling_reference_is_listed_and_an_invalid_one_is_not(board: Board) -> None:
    board.write("projects/a/questions/dangling.json", question(context_path="/projects/a/missing.md"))
    board.write("projects/a/questions/bad.json", question(session="someone"))
    output = board.call("open", {})
    assert paths(output) == ["/projects/a/questions/dangling"]
    assert output["unreadable"] == 1


def test_a_question_asked_at_a_time_python_cannot_read_is_unreadable_and_one_with_a_lowercase_z_is_listed(
    board: Board,
) -> None:
    board.write("projects/a/questions/leap.json", question(asked="2026-12-31T23:59:60Z"))
    board.write("projects/a/questions/lower.json", question(asked="2026-10-01t09:00:00z"))
    output = board.call("open", {})
    assert paths(output) == ["/projects/a/questions/lower"]
    assert output["unreadable"] == 1


def mounted(root: Path, questions: dict[str, str]) -> str:
    """A mount pointer to a new board at `root` whose question schema admits any object, holding `questions`."""
    files = {
        ".board/board.json": json.dumps({"$type": "board/config/v1"}),
        ".board/types/plans/question/v1.schema.json": json.dumps({"type": "object"}),
        **{f"projects/m/questions/{name}.json": text for name, text in questions.items()},
    }
    for path, text in files.items():
        (root / path).parent.mkdir(parents=True, exist_ok=True)
        (root / path).write_text(text)
    git = ["git", "-c", "user.name=Test", "-c", "user.email=test@example.com"]
    subprocess.run(["git", "init", "-q", "-b", "main"], cwd=root, check=True)
    subprocess.run([*git, "add", "."], cwd=root, check=True)
    subprocess.run([*git, "commit", "-qm", "questions"], cwd=root, check=True)
    commit = subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=root, check=True, capture_output=True, text=True
    ).stdout.strip()
    return json.dumps({"$type": "board/mount/v1", "repo": str(root), "follow": "main", "commit": commit})


def test_a_mounted_question_of_another_shape_is_unreadable_and_a_mounted_question_of_this_shape_is_listed(
    board: Board, tmp_path: Path
) -> None:
    misshapen = {
        "naive": question(asked="2026-10-01T09:00:00"),
        "dated": question(asked="2026-10-01"),
        "offset": question(asked="2026-10-01T09:00:00+0000"),
        "strings": question(options=["a", "b"]),
        "number": question(question=5),
    }
    board.write("lib.json", mounted(tmp_path / "lib", {"fits": question(), **misshapen}))
    output = board.call("open", {})
    assert paths(output) == ["/lib/projects/m/questions/fits"]
    assert output["unreadable"] == len(misshapen)
