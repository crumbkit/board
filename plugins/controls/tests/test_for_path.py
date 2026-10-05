"""`for-path`, through the real `board`."""

import json
import subprocess
from pathlib import Path

from conftest import Board, control


def applying(board: Board, path: str) -> list[str]:
    """The paths of the controls `board call controls.for-path` returns for `path`."""
    report = board.call("for-path", {"path": path})
    assert report["$type"] == "board/call-report/v1", report
    return [item["control"] for item in report["output"]["controls"]]


def test_a_control_applies_to_what_it_references_and_to_what_its_patterns_match(board: Board) -> None:
    board.write("AGENTS.md", "Rules.\n")
    board.write("docs/a.md", "A.\n")
    board.write("docs/b.txt", "B.\n")
    board.write("controls/agents.json", control("Stay small.", applies_to=["/AGENTS.md"]))
    board.write("controls/docs.json", control("Say what exists.", paths=["docs/*.md"]))
    assert applying(board, "/AGENTS.md") == ["/controls/agents"]
    assert applying(board, "/docs/a.md") == ["/controls/docs"]
    assert applying(board, "/docs/b.txt") == []


def test_the_output_carries_the_requirement_purpose_and_rule(board: Board) -> None:
    board.write("AGENTS.md", "Rules.\n")
    board.write(
        "controls/agents.json",
        control("Stay short.", applies_to=["/AGENTS.md"], rule={"budget": "words", "max": 10}),
    )
    report = board.call("for-path", {"path": "/AGENTS.md"})
    assert report["output"]["controls"] == [
        {
            "control": "/controls/agents",
            "requirement": "Stay short.",
            "purpose": "A test.",
            "rule": {"budget": "words", "max": 10},
        },
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


def test_a_new_file_under_a_mount_matches_no_pattern_and_one_beside_the_mount_does(
    board: Board, tmp_path: Path
) -> None:
    library = tmp_path / "library"
    (library / "docs").mkdir(parents=True)
    (library / "docs" / "a.md").write_text("A.\n")
    git = ["git", "-c", "user.name=Test", "-c", "user.email=test@example.com"]
    subprocess.run(["git", "init", "-q"], cwd=library, check=True)
    subprocess.run([*git, "add", "."], cwd=library, check=True)
    subprocess.run([*git, "commit", "-qm", "library"], cwd=library, check=True)
    commit = subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=library, check=True, capture_output=True, text=True
    ).stdout.strip()
    mount = {"$type": "board/mount/v1", "repo": str(library), "follow": "main", "commit": commit}
    board.write("vendor/lib.json", json.dumps(mount))
    board.write("controls/vendor.json", control("R.", paths=["vendor/*", "docs/*"]))
    assert applying(board, "/vendor/lib/docs/new.md") == [], "patterns match only the board's own tree"
    assert applying(board, "/vendor/new.md") == ["/controls/vendor"]


def test_a_control_the_plugin_cannot_read_is_refused_and_one_it_can_is_returned(board: Board) -> None:
    schema = board.root / ".board/types/controls/control/v1.schema.json"
    widened = json.loads(schema.read_text())
    widened["additionalProperties"] = True
    schema.write_text(json.dumps(widened))
    board.write("AGENTS.md", "Rules.\n")
    board.write("controls/kept.json", control("R.", applies_to=["/AGENTS.md"]))
    assert applying(board, "/AGENTS.md") == ["/controls/kept"]
    board.write("controls/wider.json", control("R.", applies_to=["/AGENTS.md"], owner="me"))
    refused = board.call("for-path", {"path": "/AGENTS.md"})
    assert refused["code"] == "controls::control::unreadable", refused
    assert refused["fields"]["control"] == "/controls/wider"


def test_a_malformed_path_is_refused_with_boards_code_and_a_well_formed_one_is_answered(board: Board) -> None:
    refused = board.call("for-path", {"path": "/docs/../x"})
    assert refused["code"] == "controls::board::refused", refused
    assert refused["fields"]["code"] == "board::usage::invalid"
    assert "first on PATH" not in refused["message"]
    assert applying(board, "/docs/x") == []


def test_a_control_with_one_dangling_reference_still_applies_to_the_others(board: Board) -> None:
    board.write("AGENTS.md", "Rules.\n")
    board.write("controls/kept.json", control("R.", applies_to=["/AGENTS.md", "/gone.md"]))
    assert applying(board, "/AGENTS.md") == ["/controls/kept"]
