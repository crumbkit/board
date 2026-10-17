"""`check`, through the real `board`."""

import shlex
from dataclasses import replace
from pathlib import Path
from typing import Any

import pytest
from conftest import Board, board_binary, control, library


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
    assert report["schema"] == "board/call-report/v2", report
    assert report["data"]["output"]["controls"] == [
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
    assert admitted["data"]["output"]["controls"][0]["used"] == WORDS


def test_a_control_whose_file_is_missing_fails_the_check_and_one_whose_file_exists_passes(board: Board) -> None:
    board.write(
        "controls/context.json", control("Short.", applies_to=["/AGENTS.md"], rule={"budget": "bytes", "max": 9})
    )
    refused = board.call("check", {})
    assert refused["code"] == "controls::control::degraded", refused
    assert refused["fields"]["controls"] == [{"control": "/controls/context", "code": "board::ref::dangling"}]
    board.write("AGENTS.md", "x")
    assert board.call("check", {})["data"]["output"]["controls"][0]["used"] == 1


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
    assert report["schema"] == "board/call-report/v2", report
    assert report["data"]["output"]["controls"][0]["files"] == ["/docs/a.md"]
    assert report["data"]["output"]["controls"][0]["used"] == WORDS


def test_a_pattern_reaching_into_a_mount_counts_files_there_and_one_that_does_not_counts_none(
    board: Board, tmp_path: Path
) -> None:
    board.write("vendor/lib.json", library(tmp_path / "library", {"docs/a.md": "one two three"}))
    rule = {"budget": "words", "max": WORDS}
    board.write("controls/vendor.json", control("Small.", paths=["vendor/*.md"], rule=rule))
    board.write("controls/own.json", control("Small.", paths=["docs/*.md"], rule=rule))
    report = board.call("check", {})
    assert report["schema"] == "board/call-report/v2", report
    assert [(item["control"], item["files"], item["used"]) for item in report["data"]["output"]["controls"]] == [
        ("/controls/own", [], 0),
        ("/controls/vendor", ["/vendor/lib/docs/a.md"], WORDS),
    ]


def test_a_malformed_path_in_a_mount_matches_nothing_and_a_well_formed_one_beside_it_counts(
    board: Board, tmp_path: Path
) -> None:
    board.write("vendor/lib.json", library(tmp_path / "library", {"docs/\x01.md": "one", "docs/a.md": "two"}))
    board.write("controls/vendor.json", control("Small.", paths=["vendor/*"], rule={"budget": "words", "max": WORDS}))
    report = board.call("check", {})
    assert report["schema"] == "board/call-report/v2", report
    assert report["data"]["output"]["controls"][0]["files"] == ["/vendor/lib/docs/a.md"]


def test_a_pattern_matching_a_degraded_file_fails_the_check_and_one_matching_a_whole_file_passes(board: Board) -> None:
    board.write("docs/a", "one")
    board.write("docs/a.json", "{}")
    board.write("controls/docs.json", control("Small.", paths=["docs/*"], rule={"budget": "words", "max": WORDS}))
    refused = board.call("check", {})
    assert refused["code"] == "controls::file::unreadable", refused
    assert refused["fields"]["control"] == "/controls/docs"
    assert refused["fields"]["code"] == "board::path::shadowed"
    (board.root / "docs/a.json").unlink()
    assert board.call("check", {})["data"]["output"]["controls"][0]["used"] == 1


def test_a_pattern_matching_a_file_with_no_logical_path_fails_naming_its_path_and_one_with_a_logical_path_passes(
    board: Board,
) -> None:
    board.write("docs/\x01.md", "one")
    board.write("controls/docs.json", control("Small.", paths=["docs/*"], rule={"budget": "words", "max": WORDS}))
    refused = board.call("check", {})
    assert refused["code"] == "controls::file::unreadable", refused
    assert refused["fields"]["file"] == "docs/\x01.md"
    assert refused["fields"]["code"] == "board::path::malformed"
    (board.root / "docs/\x01.md").rename(board.root / "docs/a.md")
    assert board.call("check", {})["data"]["output"]["controls"][0]["files"] == ["/docs/a.md"]


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
    assert board.call("check", {})["data"]["output"]["controls"][0]["used"] == 1


def counted(board: Board, tmp_path: Path, crash: str = "") -> tuple[Board, Path]:
    """`board`, with every `board` the plugin runs written as one line of its arguments to the returned log.

    A `board get` that names `crash` exits 1 printing nothing, as a `board` that crashed would.
    """
    log = tmp_path / "calls.log"
    wrapper = tmp_path / "wrapper"
    wrapper.mkdir()
    script = wrapper / "board"
    script.write_text(
        f'#!/bin/sh\necho "$*" >> {shlex.quote(str(log))}\n'
        f'case "$*" in *" get -- "*) case " $* " in *{shlex.quote(f" {crash} ")}*)\n'
        f"  [ -n {shlex.quote(crash)} ] && exit 1;; esac;; esac\n"
        f'exec {shlex.quote(str(board_binary()))} "$@"\n'
    )
    script.chmod(0o755)
    return replace(board, first=wrapper), log


def test_check_reads_each_file_once_and_each_controls_references_in_one_call(board: Board, tmp_path: Path) -> None:
    for name in ["a", "b", "c"]:
        board.write(f"docs/{name}.md", "one two")
    board.write("alias.json", '{"$type": "board/mount/v1", "path": "/docs"}')
    board.write(
        "controls/words.json",
        control(
            "Short.",
            applies_to=["/alias/a.md", "/docs/b.md"],
            paths=["docs/*"],
            rule={"budget": "words", "max": 6},
        ),
    )
    board.write(
        "controls/bytes.json",
        control("Small.", applies_to=["/docs/a.md", "/docs/c.md"], rule={"budget": "bytes", "max": 14}),
    )
    watched, log = counted(board, tmp_path)
    report = watched.call("check", {})
    assert [item["used"] for item in report["data"]["output"]["controls"]] == [14, 6], report
    calls = log.read_text().splitlines()
    assert [call for call in calls if " get " in call] == ["--json get -- /docs/a.md /docs/b.md /docs/c.md"]
    assert [call for call in calls if " refs " in call] == ["--json refs --all -- /controls/bytes /controls/words"]


@pytest.mark.parametrize("count", [1, 3])
def test_check_resolves_every_controls_references_in_one_refs_run(board: Board, tmp_path: Path, count: int) -> None:
    board.write("docs/a.md", "one two")
    names = [f"c{index}" for index in range(count)]
    for name in names:
        board.write(
            f"controls/{name}.json", control("Short.", applies_to=["/docs/a.md"], rule={"budget": "words", "max": 2})
        )
    board.write("controls/pattern.json", control("Short.", paths=["docs/*"], rule={"budget": "words", "max": 2}))
    watched, log = counted(board, tmp_path)
    report = watched.call("check", {})
    assert [item["used"] for item in report["data"]["output"]["controls"]] == [2] * (count + 1), report
    paths = " ".join(f"/controls/{name}" for name in names)
    assert [call for call in log.read_text().splitlines() if " refs " in call] == [f"--json refs --all -- {paths}"]


def test_check_starts_no_refs_run_when_no_control_has_references_and_one_when_a_control_has(
    board: Board, tmp_path: Path
) -> None:
    board.write("docs/a.md", "one two")
    board.write("controls/pattern.json", control("Short.", paths=["docs/*"], rule={"budget": "words", "max": 2}))
    watched, log = counted(board, tmp_path)
    assert watched.call("check", {})["data"]["output"]["controls"][0]["used"] == len(["one", "two"])
    assert [call for call in log.read_text().splitlines() if " refs " in call] == []
    board.write("controls/named.json", control("Short.", applies_to=["/docs/a.md"], rule={"budget": "words", "max": 2}))
    log.unlink()
    assert [item["used"] for item in watched.call("check", {})["data"]["output"]["controls"]] == [2, 2]
    assert [call for call in log.read_text().splitlines() if " refs " in call] == [
        "--json refs --all -- /controls/named"
    ]


def test_a_crashed_read_fails_the_check_and_without_the_crash_a_controls_unreadable_file_is_named(
    board: Board, tmp_path: Path
) -> None:
    board.write("adir/a.md", "one")
    board.write("zdir/z.md", "one")
    board.write("controls/a.json", control("Small.", applies_to=["/zdir"], rule={"budget": "words", "max": WORDS}))
    board.write("controls/b.json", control("Small.", applies_to=["/adir/a.md"], rule={"budget": "words", "max": WORDS}))
    crashing, log = counted(board, tmp_path, crash="/adir/a.md")
    failed = crashing.call("check", {})
    assert failed["code"] == "controls::board::failed", failed
    assert [call for call in log.read_text().splitlines() if " get " in call] == ["--json get -- /adir/a.md /zdir"]
    refused = board.call("check", {})
    assert refused["code"] == "controls::file::unreadable", refused
    assert (refused["fields"]["control"], refused["fields"]["file"], refused["fields"]["code"]) == (
        "/controls/a",
        "/zdir",
        "board::record::not_a_record",
    )


@pytest.mark.parametrize("count", [1, 3])
def test_check_reads_every_file_in_one_get_run(board: Board, tmp_path: Path, count: int) -> None:
    names = [f"docs/f{index}.md" for index in range(count)]
    for name in names:
        board.write(name, "one two")
    board.write("controls/pattern.json", control("Short.", paths=["docs/*"], rule={"budget": "words", "max": 99}))
    watched, log = counted(board, tmp_path)
    assert watched.call("check", {})["data"]["output"]["controls"][0]["used"] == len(names) * len(["one", "two"])
    paths = " ".join(f"/{name}" for name in names)
    assert [call for call in log.read_text().splitlines() if " get " in call] == [f"--json get -- {paths}"]
