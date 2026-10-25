"""`plans.open`: the questions still waiting for the owner."""

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
