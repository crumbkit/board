"""`check`, through the real `board`."""

from typing import Any

from conftest import Board, control


def test_check_measures_each_budget_control_over_its_files(board: Board) -> None:
    board.write("AGENTS.md", "one two\nthree\n")
    board.write("docs/a.md", "four five")
    board.write("docs/b.md", "six")
    board.write(
        "controls/context.json", control("Short.", applies_to=["/AGENTS.md"], rule={"budget": "words", "max": 3})
    )
    board.write(
        "controls/docs.json",
        control("Small.", applies_to=["/docs/a.md"], paths=["docs/*.md"], rule={"budget": "bytes", "max": 12}),
    )
    board.write("controls/plain.json", control("No budget.", paths=["*"]))
    report = board.call("check", {})
    assert report["$type"] == "board/call-report/v1", report
    assert report["output"]["controls"] == [
        {
            "control": "/controls/context",
            "requirement": "Short.",
            "budget": "words",
            "max": 3,
            "used": 3,
            "files": ["/AGENTS.md"],
        },
        {
            "control": "/controls/docs",
            "requirement": "Small.",
            "budget": "bytes",
            "max": 12,
            "used": 12,
            "files": ["/docs/a.md", "/docs/b.md"],
        },
    ]


WORDS = 3


def over(board: Board, max_words: int) -> dict[str, Any]:
    """`check` on a board whose one control allows `max_words` of a three-word file."""
    board.write("AGENTS.md", "one two three")
    board.write(
        "controls/context.json",
        control("Short.", applies_to=["/AGENTS.md"], rule={"budget": "words", "max": max_words}),
    )
    return board.call("check", {})


def test_a_control_over_its_budget_fails_the_check_and_one_at_it_passes(board: Board) -> None:
    refused = over(board, 2)
    assert refused["code"] == "controls::budget::exceeded", refused
    assert "/controls/context (3 of 2 words)" in refused["message"]
    assert refused["fields"]["controls"][0]["used"] == WORDS
    admitted = over(board, WORDS)
    assert admitted["output"]["controls"][0]["used"] == WORDS


def test_a_control_whose_file_is_missing_fails_the_check_and_one_whose_file_exists_passes(board: Board) -> None:
    board.write(
        "controls/context.json", control("Short.", applies_to=["/AGENTS.md"], rule={"budget": "bytes", "max": 9})
    )
    refused = board.call("check", {})
    assert refused["code"] == "controls::control::degraded", refused
    assert refused["fields"]["controls"] == [{"control": "/controls/context", "code": "board::ref::dangling"}]
    board.write("AGENTS.md", "x")
    assert board.call("check", {})["output"]["controls"][0]["used"] == 1


def test_a_file_reached_through_a_link_and_by_pattern_counts_once_and_a_link_counts_nothing(board: Board) -> None:
    board.write("docs/a.md", "one two three")
    board.write("alias.json", '{"$type": "board/mount/v1", "path": "/docs"}')
    board.write(
        "controls/docs.json",
        control(
            "Small.", applies_to=["/alias/a.md"], paths=["docs/*", "alias*"], rule={"budget": "words", "max": WORDS}
        ),
    )
    report = board.call("check", {})
    assert report["$type"] == "board/call-report/v1", report
    assert report["output"]["controls"][0]["files"] == ["/docs/a.md"]
    assert report["output"]["controls"][0]["used"] == WORDS


def test_a_pattern_matching_a_degraded_file_fails_the_check_and_one_matching_a_whole_file_passes(board: Board) -> None:
    board.write("docs/a", "one")
    board.write("docs/a.json", "{}")
    board.write("controls/docs.json", control("Small.", paths=["docs/*"], rule={"budget": "words", "max": WORDS}))
    refused = board.call("check", {})
    assert refused["code"] == "controls::file::unreadable", refused
    assert refused["fields"]["control"] == "/controls/docs"
    assert refused["fields"]["code"] == "board::path::shadowed"
    (board.root / "docs/a.json").unlink()
    assert board.call("check", {})["output"]["controls"][0]["used"] == 1


def test_a_reference_to_a_directory_fails_the_check_and_one_to_a_file_passes(board: Board) -> None:
    board.write("docs/a.md", "one")
    board.write("controls/docs.json", control("Small.", applies_to=["/docs"], rule={"budget": "words", "max": WORDS}))
    refused = board.call("check", {})
    assert refused["code"] == "controls::file::unreadable", refused
    assert refused["fields"]["file"] == "/docs"
    assert "first on PATH" not in refused["message"]
    board.write(
        "controls/docs.json", control("Small.", applies_to=["/docs/a.md"], rule={"budget": "words", "max": WORDS})
    )
    assert board.call("check", {})["output"]["controls"][0]["used"] == 1
