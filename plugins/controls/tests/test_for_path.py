"""`for-path`, through the real `board`."""

import json
from pathlib import Path

from conftest import Board, control, library


def applying(board: Board, path: str) -> list[str]:
    """The paths of the controls `board call controls.for-path` returns for `path`."""
    report = board.call("for-path", {"path": path})
    assert report["schema"] == "board/call-report/v2", report
    return [item["control"] for item in report["data"]["output"]["controls"]]


def test_a_control_applies_to_what_it_references_and_to_what_its_patterns_match(board: Board) -> None:
    board.write("AGENTS.md", "Rules.\n")
    board.write("docs/a.md", "A.\n")
    board.write("docs/b.txt", "B.\n")
    board.write("controls/agents.json", control("Stay small.", applies_to=["/AGENTS.md"]))
    board.write("controls/docs.json", control("Say what exists.", paths=["docs/*.md"]))
    assert applying(board, "/AGENTS.md") == ["/controls/agents"]
    assert applying(board, "/docs/a.md") == ["/controls/docs"]
    assert applying(board, "/docs/b.txt") == []


def test_the_output_carries_the_requirement_purpose_and_rule_and_null_for_a_missing_rule(board: Board) -> None:
    board.write("AGENTS.md", "Rules.\n")
    board.write(
        "controls/agents.json",
        control("Stay short.", applies_to=["/AGENTS.md"], rule={"budget": "words", "max": 10}),
    )
    board.write("controls/kind.json", control("Be kind.", applies_to=["/AGENTS.md"]))
    report = board.call("for-path", {"path": "/AGENTS.md"})
    assert report["data"]["output"]["controls"] == [
        {
            "control": "/controls/agents",
            "requirement": "Stay short.",
            "purpose": "A test.",
            "rule": {"budget": "words", "max": 10},
        },
        {"control": "/controls/kind", "requirement": "Be kind.", "purpose": "A test.", "rule": None},
    ]


def test_an_invalid_control_applies_to_nothing_and_its_valid_twin_applies(board: Board) -> None:
    board.write("AGENTS.md", "Rules.\n")
    board.write("controls/broken.json", control("R.", applies_to=["/AGENTS.md"], extra=True))
    board.write("controls/kept.json", control("R.", applies_to=["/AGENTS.md"]))
    assert applying(board, "/AGENTS.md") == ["/controls/kept"]


def test_a_path_with_a_quote_is_matched_as_written(board: Board) -> None:
    board.write("docs/it's.md", "A.\n")
    board.write("controls/docs.json", control("R.", paths=["docs/*"]))
    assert applying(board, "/docs/it's.md") == ["/controls/docs"]


def test_a_path_with_no_file_yet_is_matched_by_pattern_and_a_record_by_its_file(board: Board) -> None:
    board.write("controls/docs.json", control("Say what exists.", paths=["docs/*.md", "tasks/*.json"]))
    board.write("tasks/a.json", "{}")
    assert applying(board, "/docs/new.md") == ["/controls/docs"]
    assert applying(board, "/docs/new.txt") == []
    assert applying(board, "/tasks/a") == ["/controls/docs"]


def test_a_path_asked_through_a_link_is_matched_where_it_leads(board: Board) -> None:
    board.write("docs/a.md", "A.\n")
    board.write("alias.json", '{"$type": "board/mount/v1", "path": "/docs"}')
    board.write("controls/docs.json", control("R.", paths=["docs/*.md"]))
    assert applying(board, "/alias/a.md") == ["/controls/docs"]
    assert applying(board, "/docs/a.md") == ["/controls/docs"]


def test_a_new_file_asked_through_a_link_is_matched_where_it_would_be_written(board: Board) -> None:
    board.write("docs/a.md", "A.\n")
    board.write("alias.json", '{"$type": "board/mount/v1", "path": "/docs"}')
    board.write("controls/docs.json", control("R.", paths=["docs/*.md"]))
    assert applying(board, "/alias/new.md") == ["/controls/docs"]
    assert applying(board, "/alias/sub/new.md") == ["/controls/docs"]
    assert applying(board, "/alias/new.txt") == []
    assert applying(board, "/alias/a.md/new.md") == [], "nothing can be beneath a file"


def test_a_pattern_reaching_into_a_mount_matches_files_there_and_one_that_does_not_matches_none(
    board: Board, tmp_path: Path
) -> None:
    board.write("vendor/lib.json", library(tmp_path / "library", {"docs/a.md": "A.\n", "tasks/a.json": "{}"}))
    board.write("controls/vendor.json", control("R.", paths=["vendor/*.md", "vendor/*/tasks/*.json"]))
    board.write("controls/own.json", control("R.", paths=["docs/*", "tasks/*"]))
    assert applying(board, "/vendor/lib/docs/a.md") == ["/controls/vendor"]
    assert applying(board, "/vendor/lib/docs/new.md") == ["/controls/vendor"], "matched where it would be"
    assert applying(board, "/vendor/lib/tasks/a") == ["/controls/vendor"], "a record is matched by its file"
    assert applying(board, "/vendor/lib/docs/a.txt") == []
    board.write("controls/beside.json", control("R.", paths=["vendor/*.md"]))
    board.write("controls/vendor.json", control("R.", paths=["vendor/lib/*"]))
    assert applying(board, "/vendor/new.md") == ["/controls/beside"], "an own file beside the mount"


def test_a_file_in_a_commit_mounted_twice_is_matched_by_the_path_check_counts_it_under(
    board: Board, tmp_path: Path
) -> None:
    pointer = library(tmp_path / "library", {"docs/a.md": "one"})
    board.write("aa/lib.json", pointer)
    board.write("zz/long/lib.json", pointer)
    rule = {"budget": "words", "max": 9}
    board.write("controls/short.json", control("R.", paths=["aa/*"], rule=rule))
    board.write("controls/long.json", control("R.", paths=["zz/*"], rule=rule))
    checked = board.call("check", {})["data"]["output"]["measured"]
    assert [(item["control"], item["files"]) for item in checked] == [
        ("/controls/long", []),
        ("/controls/short", ["/aa/lib/docs/a.md"]),
    ]
    assert applying(board, "/zz/long/lib/docs/a.md") == ["/controls/short"]
    assert applying(board, "/aa/lib/docs/a.md") == ["/controls/short"]
    assert applying(board, "/zz/long/lib/docs/new.md") == ["/controls/long"], "a missing path is matched as asked"


def test_a_directory_named_json_is_matched_by_its_name_and_a_json_file_or_symlink_by_its_file_name(
    board: Board,
) -> None:
    board.write("docs/t.md", "T.\n")
    board.write("docs/a.json", "{}")
    (board.root / "docs/l.json").symlink_to("t.md")
    board.write("box.json/f.md", "F.\n")
    board.write("controls/names.json", control("R.", paths=["docs/l.json", "docs/a.json", "box.json"]))
    board.write("controls/doubled.json", control("R.", paths=["*.json.json"]))
    assert applying(board, "/docs/l") == ["/controls/names"]
    assert applying(board, "/docs/a") == ["/controls/names"]
    assert applying(board, "/box.json") == ["/controls/names"]


def test_board_refuses_the_call_when_its_control_schema_is_wider_and_answers_with_the_shipped_one(board: Board) -> None:
    board.write("AGENTS.md", "Rules.\n")
    board.write("controls/kept.json", control("R.", applies_to=["/AGENTS.md"]))
    assert applying(board, "/AGENTS.md") == ["/controls/kept"]
    assert board.call("check", {})["schema"] == "board/call-report/v2"
    schema = board.root / ".board/types/controls/control/v1.schema.json"
    widened = json.loads(schema.read_text())
    widened["additionalProperties"] = True
    schema.write_text(json.dumps(widened))
    for capability, given in [("for-path", {"path": "/AGENTS.md"}), ("check", {})]:
        refused = board.call(capability, given)
        assert refused["code"] == "board::plugin::schema_differs", refused
        assert refused["fields"]["schema"] == ".board/types/controls/control/v1.schema.json"


def test_a_malformed_path_is_refused_with_boards_code_and_a_well_formed_one_is_answered(board: Board) -> None:
    refused = board.call("for-path", {"path": "/docs/../x"})
    assert refused["code"] == "controls::board::refused", refused
    assert refused["fields"]["code"] == "board::cli::usage"
    assert "first on PATH" not in refused["message"]
    assert applying(board, "/docs/x") == []


def test_a_control_whose_malformed_reference_hides_a_schema_error_is_left_out_and_a_dangling_one_applies(
    board: Board,
) -> None:
    board.write("controls/broken.json", control("R.", applies_to=["docs"], paths=["docs/*"], purpose=5))
    board.write("controls/kept.json", control("R.", applies_to=["/gone.md"], paths=["docs/*"]))
    assert applying(board, "/docs/a.md") == ["/controls/kept"]


def test_a_control_with_one_dangling_reference_still_applies_to_the_others(board: Board) -> None:
    board.write("AGENTS.md", "Rules.\n")
    board.write("controls/kept.json", control("R.", applies_to=["/AGENTS.md", "/gone.md"]))
    assert applying(board, "/AGENTS.md") == ["/controls/kept"]
