"""`check`, through the real `board`."""

import json
import shlex
from dataclasses import replace
from pathlib import Path
from typing import Any

import pytest
from conftest import Board, board_binary, control, library

# What `check` adds to a control in the board's own tree that names no step and no basis.
UNSCOPED: dict[str, Any] = {"board": "/", "adds_step": None, "basis": []}


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
    assert report["data"]["output"]["measured"] == [
        {
            "control": "/controls/context",
            "requirement": "Short.",
            "budget": "words",
            "max": 3,
            "used": 3,
            "files": ["/AGENTS.md"],
            **UNSCOPED,
        },
        {
            "control": "/controls/docs",
            "requirement": "Small.",
            "budget": "bytes",
            "max": 12,
            "used": 12,
            "files": ["/docs/a.md", "/docs/b.md"],
            **UNSCOPED,
        },
    ]


WORDS = 3
BOTH = 5


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
    assert admitted["data"]["output"]["measured"][0]["used"] == WORDS


def test_a_control_whose_file_is_missing_fails_the_check_and_one_whose_file_exists_passes(board: Board) -> None:
    board.write(
        "controls/context.json", control("Short.", applies_to=["/AGENTS.md"], rule={"budget": "bytes", "max": 9})
    )
    refused = board.call("check", {})
    assert refused["code"] == "controls::control::degraded", refused
    assert refused["fields"]["controls"] == [{"control": "/controls/context", "code": "board::ref::dangling"}]
    board.write("AGENTS.md", "x")
    assert board.call("check", {})["data"]["output"]["measured"][0]["used"] == 1


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
    assert report["data"]["output"]["measured"][0]["files"] == ["/docs/a.md"]
    assert report["data"]["output"]["measured"][0]["used"] == WORDS


def test_a_pattern_reaching_into_a_mount_counts_files_there_and_one_that_does_not_counts_none(
    board: Board, tmp_path: Path
) -> None:
    board.write("vendor/lib.json", library(tmp_path / "library", {"docs/a.md": "one two three"}))
    rule = {"budget": "words", "max": WORDS}
    board.write("controls/vendor.json", control("Small.", paths=["vendor/*.md"], rule=rule))
    board.write("controls/own.json", control("Small.", paths=["docs/*.md"], rule=rule))
    report = board.call("check", {})
    assert report["schema"] == "board/call-report/v2", report
    assert [(item["control"], item["files"], item["used"]) for item in report["data"]["output"]["measured"]] == [
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
    assert report["data"]["output"]["measured"][0]["files"] == ["/vendor/lib/docs/a.md"]


def test_a_pattern_matching_a_degraded_file_fails_the_check_and_one_matching_a_whole_file_passes(board: Board) -> None:
    board.write("docs/a", "one")
    board.write("docs/a.json", "{}")
    board.write("controls/docs.json", control("Small.", paths=["docs/*"], rule={"budget": "words", "max": WORDS}))
    refused = board.call("check", {})
    assert refused["code"] == "controls::file::unreadable", refused
    assert refused["fields"]["control"] == "/controls/docs"
    assert refused["fields"]["code"] == "board::path::shadowed"
    (board.root / "docs/a.json").unlink()
    assert board.call("check", {})["data"]["output"]["measured"][0]["used"] == 1


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
    assert board.call("check", {})["data"]["output"]["measured"][0]["files"] == ["/docs/a.md"]


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
    assert board.call("check", {})["data"]["output"]["measured"][0]["used"] == 1


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
    assert [item["used"] for item in report["data"]["output"]["measured"]] == [14, 6], report
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
    assert [item["used"] for item in report["data"]["output"]["measured"]] == [2] * (count + 1), report
    paths = " ".join(f"/controls/{name}" for name in names)
    assert [call for call in log.read_text().splitlines() if " refs " in call] == [f"--json refs --all -- {paths}"]


def test_check_starts_no_refs_run_when_no_control_has_references_and_one_when_a_control_has(
    board: Board, tmp_path: Path
) -> None:
    board.write("docs/a.md", "one two")
    board.write("controls/pattern.json", control("Short.", paths=["docs/*"], rule={"budget": "words", "max": 2}))
    watched, log = counted(board, tmp_path)
    assert watched.call("check", {})["data"]["output"]["measured"][0]["used"] == len(["one", "two"])
    assert [call for call in log.read_text().splitlines() if " refs " in call] == []
    board.write("controls/named.json", control("Short.", applies_to=["/docs/a.md"], rule={"budget": "words", "max": 2}))
    log.unlink()
    assert [item["used"] for item in watched.call("check", {})["data"]["output"]["measured"]] == [2, 2]
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
    assert watched.call("check", {})["data"]["output"]["measured"][0]["used"] == len(names) * len(["one", "two"])
    paths = " ".join(f"/{name}" for name in names)
    assert [call for call in log.read_text().splitlines() if " get " in call] == [f"--json get -- {paths}"]


def scoped(board: Board, tmp_path: Path) -> None:
    """A board with a budget and a stated control under /docs, the same two under /src, and one in a mount."""
    board.write("docs/a.md", "one two")
    board.write("src/a.rs", "three")
    board.write("vendor/lib.json", library(tmp_path / "library", {"README.md": "four"}))
    board.write("alias.json", '{"$type": "board/mount/v1", "path": "/docs"}')
    rule = {"budget": "words", "max": WORDS}
    board.write("controls/docs-budget.json", control("Short docs.", paths=["docs/*"], rule=rule))
    board.write("controls/docs-style.json", control("Plain docs.", applies_to=["/docs/a.md"]))
    board.write("controls/src-budget.json", control("Short code.", paths=["src/*"], rule=rule))
    board.write("controls/src-style.json", control("Plain code.", paths=["src/*"]))
    board.write("controls/vendor.json", control("Leave vendored files.", paths=["vendor/*"]))


def names(report: dict[str, Any]) -> tuple[list[str], list[str]]:
    """The measured and the stated controls of a `check` report, by path."""
    assert report["schema"] == "board/call-report/v2", report
    output = report["data"]["output"]
    return [item["control"] for item in output["measured"]], [item["control"] for item in output["stated"]]


def test_a_path_reports_the_controls_under_it_and_leaves_out_their_twins_elsewhere(
    board: Board, tmp_path: Path
) -> None:
    scoped(board, tmp_path)
    report = board.call("check", {"path": "/docs"})
    assert names(report) == (["/controls/docs-budget"], ["/controls/docs-style"])
    assert report["data"]["output"]["stated"] == [
        {
            "control": "/controls/docs-style",
            "requirement": "Plain docs.",
            "purpose": "A test.",
            "paths": [],
            "files": 1,
            **UNSCOPED,
        }
    ]
    assert report["data"]["output"]["measured"][0]["used"] == 2  # noqa: PLR2004 - the words in docs/a.md
    assert names(board.call("check", {"path": "/src"})) == (["/controls/src-budget"], ["/controls/src-style"])


def test_check_without_a_path_reports_every_control_as_the_root_does(board: Board, tmp_path: Path) -> None:
    scoped(board, tmp_path)
    everything = (
        ["/controls/docs-budget", "/controls/src-budget"],
        ["/controls/docs-style", "/controls/src-style", "/controls/vendor"],
    )
    assert names(board.call("check", {})) == everything
    assert names(board.call("check", {"path": "/"})) == everything


def test_a_path_into_a_mount_reports_its_controls_and_one_beside_the_mount_does_not(
    board: Board, tmp_path: Path
) -> None:
    scoped(board, tmp_path)
    report = board.call("check", {"path": "/vendor/lib"})
    assert names(report) == ([], ["/controls/vendor"])
    assert report["data"]["output"]["stated"][0]["files"] == 1
    assert names(board.call("check", {"path": "/vendor"})) == ([], ["/controls/vendor"])
    assert names(board.call("check", {"path": "/controls"})) == ([], [])


def test_a_path_through_a_link_is_checked_where_it_leads(board: Board, tmp_path: Path) -> None:
    scoped(board, tmp_path)
    assert names(board.call("check", {"path": "/alias"})) == names(board.call("check", {"path": "/docs"}))


def test_a_path_with_nothing_there_is_refused_and_one_with_a_file_there_is_admitted(
    board: Board, tmp_path: Path
) -> None:
    scoped(board, tmp_path)
    refused = board.call("check", {"path": "/nowhere"})
    assert refused["code"] == "controls::board::refused", refused
    assert refused["fields"]["code"] == "board::path::not_found"
    assert "nothing is at /nowhere" in refused["message"]
    assert names(board.call("check", {"path": "/docs/a.md"})) == (["/controls/docs-budget"], ["/controls/docs-style"])


def test_a_budget_over_its_max_fails_a_path_it_applies_under_and_not_one_it_does_not(
    board: Board, tmp_path: Path
) -> None:
    scoped(board, tmp_path)
    board.write("src/b.rs", "five six seven")
    over = board.run("--json", "call", "controls.check", "--path", "/src")
    assert over.returncode == 1, over.stdout
    refused = json.loads(over.stdout)
    assert refused["code"] == "controls::budget::exceeded", refused
    assert [item["control"] for item in refused["fields"]["controls"]] == ["/controls/src-budget"]
    within = board.run("--json", "call", "controls.check", "--path", "/docs")
    assert within.returncode == 0, within.stdout
    assert names(json.loads(within.stdout)) == (["/controls/docs-budget"], ["/controls/docs-style"])


def test_a_degraded_control_fails_a_path_it_does_not_reach_and_the_same_control_whole_passes(
    board: Board, tmp_path: Path
) -> None:
    scoped(board, tmp_path)
    board.write("controls/src-file.json", control("Plain code.", applies_to=["/src/b.rs"]))
    refused = board.call("check", {"path": "/docs"})
    assert refused["code"] == "controls::control::degraded", refused
    assert refused["fields"]["controls"] == [{"control": "/controls/src-file", "code": "board::ref::dangling"}]
    board.write("src/b.rs", "five")
    assert names(board.call("check", {"path": "/docs"})) == (["/controls/docs-budget"], ["/controls/docs-style"])


def test_a_stated_control_reaching_an_unreadable_file_passes_and_a_budget_reaching_it_fails(board: Board) -> None:
    board.write("docs/a", "one")
    board.write("docs/a.json", "{}")
    board.write("controls/style.json", control("Plain.", paths=["docs/*"]))
    stated = board.call("check", {"path": "/docs"})
    assert stated["schema"] == "board/call-report/v2", stated
    assert [item["control"] for item in stated["data"]["output"]["stated"]] == ["/controls/style"]
    board.write("controls/budget.json", control("Small.", paths=["docs/*"], rule={"budget": "words", "max": WORDS}))
    refused = board.call("check", {"path": "/docs"})
    assert refused["code"] == "controls::file::unreadable", refused
    assert refused["fields"]["control"] == "/controls/budget"


def test_a_budget_lists_its_files_outside_the_path_and_a_stated_control_counts_only_those_inside(
    board: Board, tmp_path: Path
) -> None:
    scoped(board, tmp_path)
    board.write("controls/both.json", control("Both.", paths=["docs/*", "src/*"], rule={"budget": "words", "max": 9}))
    board.write("controls/both-style.json", control("Both.", paths=["docs/*", "src/*"]))
    output = board.call("check", {"path": "/docs"})["data"]["output"]
    assert [(item["files"], item["used"]) for item in output["measured"] if item["control"] == "/controls/both"] == [
        (["/docs/a.md", "/src/a.rs"], WORDS)
    ]
    assert [
        (item["paths"], item["files"]) for item in output["stated"] if item["control"] == "/controls/both-style"
    ] == [(["docs/*", "src/*"], 1)]


def test_the_root_lists_a_control_that_reaches_no_file_and_a_narrower_path_leaves_it_out(
    board: Board, tmp_path: Path
) -> None:
    scoped(board, tmp_path)
    board.write("controls/typo.json", control("Typo.", paths=["dosc/*"], rule={"budget": "words", "max": WORDS}))
    board.write("controls/typo-style.json", control("Typo.", paths=["dosc/*"]))
    report = board.call("check", {})
    assert [(item["control"], item["files"], item["used"]) for item in report["data"]["output"]["measured"]][-1] == (
        "/controls/typo",
        [],
        0,
    )
    assert [
        item["files"] for item in report["data"]["output"]["stated"] if item["control"] == "/controls/typo-style"
    ] == [0]
    assert names(board.call("check", {"path": "/docs"})) == (["/controls/docs-budget"], ["/controls/docs-style"])


def test_a_stated_control_counts_a_file_it_reaches_by_reference_and_pattern_once_and_another_file_again(
    board: Board, tmp_path: Path
) -> None:
    scoped(board, tmp_path)
    board.write("controls/twice.json", control("Twice.", applies_to=["/docs/a.md"], paths=["docs/*"]))

    def counted() -> list[tuple[list[str], int]]:
        output = board.call("check", {"path": "/docs"})["data"]["output"]
        return [(item["paths"], item["files"]) for item in output["stated"] if item["control"] == "/controls/twice"]

    assert counted() == [(["docs/*"], 1)]
    board.write("docs/b.md", "eight")
    assert counted() == [(["docs/*"], 2)]


def per(board: Board, mode: str | None, max_words: int) -> dict[str, Any]:
    """`check` on a three-word and a two-word file, `BOTH` words in all, under a control of `max_words` `per` `mode`."""
    board.write("docs/a.md", "one two three")
    board.write("docs/b.md", "four five")
    rule: dict[str, Any] = {"budget": "words", "max": max_words}
    if mode is not None:
        rule["per"] = mode
    board.write("controls/docs.json", control("Small.", paths=["docs/*.md"], rule=rule))
    return board.call("check", {})


def test_a_per_file_control_passes_when_each_file_is_within_it_and_a_total_one_sums_the_same_files(
    board: Board,
) -> None:
    admitted = per(board, "file", WORDS)
    assert admitted["data"]["output"]["measured"] == [
        {
            "control": "/controls/docs",
            "requirement": "Small.",
            "budget": "words",
            "max": WORDS,
            "used": WORDS,
            "files": ["/docs/a.md", "/docs/b.md"],
            "per": "file",
            "largest": "/docs/a.md",
            "over": [],
            **UNSCOPED,
        }
    ], admitted
    summed = per(board, "total", BOTH)
    assert summed["data"]["output"]["measured"][0]["used"] == BOTH, summed
    assert summed["data"]["output"]["measured"][0]["files"] == ["/docs/a.md", "/docs/b.md"]
    assert per(board, "total", WORDS)["code"] == "controls::budget::exceeded"


def test_a_per_file_control_fails_naming_each_file_over_it_and_passes_once_that_file_is_within_it(
    board: Board,
) -> None:
    refused = per(board, "file", 2)
    assert refused["code"] == "controls::budget::exceeded", refused
    assert "/controls/docs (each file at most 2 words: /docs/a.md has 3)" in refused["message"]
    [failing] = refused["fields"]["controls"]
    assert (failing["used"], failing["largest"]) == (WORDS, "/docs/a.md")
    assert failing["over"] == [{"file": "/docs/a.md", "used": WORDS}]
    board.write("docs/a.md", "one two")
    assert board.call("check", {})["data"]["output"]["measured"][0]["over"] == []


def test_a_per_file_control_names_the_first_of_equal_files_as_largest_and_lists_every_file_over_it(
    board: Board,
) -> None:
    per(board, "file", 2)
    board.write("docs/b.md", "four five six")
    refused = board.call("check", {})
    assert refused["code"] == "controls::budget::exceeded", refused
    assert "/controls/docs (each file at most 2 words: /docs/a.md has 3, /docs/b.md has 3)" in refused["message"]
    [failing] = refused["fields"]["controls"]
    assert (failing["used"], failing["largest"]) == (WORDS, "/docs/a.md")
    assert failing["over"] == [{"file": "/docs/a.md", "used": WORDS}, {"file": "/docs/b.md", "used": WORDS}]


def test_a_control_naming_per_total_measures_exactly_as_one_naming_no_per(board: Board) -> None:
    named = per(board, "total", BOTH)
    unnamed = per(board, None, BOTH)
    assert named["data"]["output"] == unnamed["data"]["output"], (named, unnamed)
    assert set(unnamed["data"]["output"]["measured"][0]) == {
        "control",
        "requirement",
        "budget",
        "max",
        "used",
        "files",
        *UNSCOPED,
    }


def test_a_per_file_control_with_no_files_measures_nothing_as_its_largest(board: Board) -> None:
    board.write(
        "controls/docs.json", control("Small.", paths=["docs/*.md"], rule={"budget": "words", "max": 1, "per": "file"})
    )
    [measured] = board.call("check", {})["data"]["output"]["measured"]
    assert (measured["used"], measured["largest"], measured["over"]) == (0, None, [])
