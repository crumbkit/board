"""`changes.plan`: a change's plan, the records it uses, and the limits on both."""

import json
from typing import Any

from conftest import PATHS, PLAN, Board, change, record

from board_changes.plan import PLAN_WORDS, USES_WORDS

PATH = "/changes/t"


def words(count: int) -> str:
    """A Markdown document of `count` words under one heading word."""
    return "#" + " w" * count + "\n"


def plans(board: Board) -> list[str]:
    """The files under the change's plans directory, by name."""
    return sorted(path.name for path in (board.root / "changes/t/plans").iterdir())


def test_plan_writes_the_plan_and_its_text_and_points_the_change_at_the_text(board: Board) -> None:
    board.write("changes/t.json", change())
    board.write("standards/errors.md", "# Errors\n")
    output = board.call(
        "plan", {"path": PATH, "text": PLAN, "uses": ["/standards/errors.md"], "paths": PATHS}, stdin=True
    )
    stamp = output["path"].removeprefix("/changes/t/plans/")
    assert output == {"path": f"/changes/t/plans/{stamp}", "text": f"/changes/t/plans/{stamp}.md", "uses": 1}
    assert board.read(f"changes/t/plans/{stamp}.json") == {
        "$type": "changes/plan/v1",
        "text": f"/changes/t/plans/{stamp}.md",
        "uses": ["/standards/errors.md"],
        "paths": PATHS,
    }
    assert (board.root / f"changes/t/plans/{stamp}.md").read_text() == PLAN
    assert board.read("changes/t.json")["plan"] == f"/changes/t/plans/{stamp}.md"


def test_a_second_plan_is_written_beside_the_first_and_becomes_the_change_s_plan(board: Board) -> None:
    board.write("changes/t.json", change())
    first = board.call("plan", {"path": PATH, "text": PLAN, "uses": [], "paths": PATHS}, stdin=True)
    second = board.call("plan", {"path": PATH, "text": PLAN + "More.\n", "uses": [], "paths": PATHS}, stdin=True)
    assert first["path"] < second["path"]
    assert len(plans(board)) == 2 * 2  # two plans, each a record and its text
    assert board.read("changes/t.json")["plan"] == second["text"]


def test_a_plan_that_uses_a_path_holding_nothing_is_refused_and_writes_nothing(board: Board) -> None:
    board.write("changes/t.json", change())
    board.write("standards/errors.md", "# Errors\n")
    refused = board.refusal(
        "plan",
        {"path": PATH, "text": PLAN, "uses": ["/standards/errors.md", "/standards/none"], "paths": PATHS},
        stdin=True,
    )
    assert refused["code"] == "changes::ref::dangling"
    assert refused["fields"]["refs"] == ["/standards/none"]
    assert not (board.root / "changes/t/plans").exists()
    assert (
        board.call("plan", {"path": PATH, "text": PLAN, "uses": ["/standards/errors.md"], "paths": PATHS}, stdin=True)[
            "uses"
        ]
        == 1
    )


def test_a_plan_over_3000_words_is_refused_and_one_of_3000_is_written(board: Board) -> None:
    board.write("changes/t.json", change())
    refused = board.refusal("plan", {"path": PATH, "text": words(PLAN_WORDS), "uses": [], "paths": PATHS}, stdin=True)
    assert refused["code"] == "changes::plan::too_long"
    assert refused["fields"]["words"] == PLAN_WORDS + 1
    assert (
        board.call("plan", {"path": PATH, "text": words(PLAN_WORDS - 1), "uses": [], "paths": PATHS}, stdin=True)[
            "uses"
        ]
        == 0
    )


def test_documents_used_over_3000_words_in_all_are_refused_and_3000_are_admitted(board: Board) -> None:
    board.write("changes/t.json", change())
    board.write("standards/a.md", words(1499))
    board.write("standards/b.md", words(1500))
    board.write("decisions/d.json", record("plans/decision/v1", decision=" w" * 4000))
    uses = ["/standards/a.md", "/standards/b.md", "/decisions/d"]
    refused = board.refusal("plan", {"path": PATH, "text": PLAN, "uses": uses, "paths": PATHS}, stdin=True)
    assert refused["code"] == "changes::plan::uses_too_long"
    assert refused["fields"]["words"] == USES_WORDS + 1
    board.write("standards/b.md", words(1499))
    assert board.call("plan", {"path": PATH, "text": PLAN, "uses": uses, "paths": PATHS}, stdin=True)["uses"] == len(
        uses
    )


def test_a_plan_for_a_done_change_is_refused_and_one_for_a_change_in_build_is_written(board: Board) -> None:
    board.write("changes/t.json", change("done"))
    assert (
        board.refusal("plan", {"path": PATH, "text": PLAN, "uses": [], "paths": PATHS}, stdin=True)["code"]
        == "changes::phase::final"
    )
    board.write("changes/t.json", change("build"))
    assert board.call("plan", {"path": PATH, "text": PLAN, "uses": [], "paths": PATHS}, stdin=True)["uses"] == 0


def test_a_plan_that_uses_a_directory_is_refused_and_one_using_a_document_in_it_is_written(board: Board) -> None:
    board.write("changes/t.json", change())
    board.write("standards/errors.md", "# Errors\n")
    refused = board.refusal("plan", {"path": PATH, "text": PLAN, "uses": ["/standards"], "paths": PATHS}, stdin=True)
    assert (refused["code"], refused["fields"]["refs"]) == ("changes::plan::directory", ["/standards"])
    assert (
        board.call("plan", {"path": PATH, "text": PLAN, "uses": ["/standards/errors.md"], "paths": PATHS}, stdin=True)[
            "uses"
        ]
        == 1
    )


def test_a_used_record_that_fails_its_schema_is_refused_and_one_that_keeps_it_is_written(board: Board) -> None:
    board.write("changes/t.json", change())
    board.write(".board/types/x/thing/v1.schema.json", '{"type": "object", "required": ["a"]}')
    board.write("things/t.json", record("x/thing/v1"))
    refused = board.refusal("plan", {"path": PATH, "text": PLAN, "uses": ["/things/t"], "paths": PATHS}, stdin=True)
    assert (refused["code"], refused["fields"]["refs"]) == ("changes::ref::dangling", ["/things/t"])
    board.write("things/t.json", record("x/thing/v1", a=1))
    assert (
        board.call("plan", {"path": PATH, "text": PLAN, "uses": ["/things/t"], "paths": PATHS}, stdin=True)["uses"] == 1
    )


def test_a_reference_given_twice_is_kept_once(board: Board) -> None:
    board.write("changes/t.json", change())
    board.write("standards/errors.md", "# Errors\n")
    uses = ["/standards/errors.md", "/standards/errors.md"]
    output = board.call("plan", {"path": PATH, "text": PLAN, "uses": uses, "paths": PATHS}, stdin=True)
    assert board.read(output["path"].removeprefix("/") + ".json")["uses"] == ["/standards/errors.md"]


def test_a_plan_of_only_whitespace_is_refused_and_one_word_is_written(board: Board) -> None:
    board.write("changes/t.json", change())
    refused = board.refusal("plan", {"path": PATH, "text": "  \n ", "uses": [], "paths": PATHS}, stdin=True)
    assert refused["code"] == "board::plugin::bad_input"
    assert board.call("plan", {"path": PATH, "text": "Plan.", "uses": [], "paths": PATHS}, stdin=True)["uses"] == 0


def test_a_used_record_degraded_only_by_a_broken_reference_of_its_own_is_written(board: Board) -> None:
    board.write("changes/t.json", change())
    board.write(
        ".board/types/x/link/v1.schema.json", '{"type": "object", "properties": {"to": {"format": "board-ref"}}}'
    )
    board.write("links/l.json", record("x/link/v1", to="/missing"))
    assert (
        board.call("plan", {"path": PATH, "text": PLAN, "uses": ["/links/l"], "paths": PATHS}, stdin=True)["uses"] == 1
    )


def test_a_plan_for_a_build_change_without_paths_is_refused_and_one_with_paths_is_written(board: Board) -> None:
    board.write("changes/t.json", change())
    unplanned: list[dict[str, Any]] = [
        {"path": PATH, "text": PLAN, "uses": []},
        {"path": PATH, "text": PLAN, "uses": [], "paths": []},
    ]
    for given in unplanned:
        refused = board.refusal("plan", given, stdin=True)
        assert refused["code"] == "changes::plan::no_paths", refused
    assert not (board.root / "changes/t/plans").exists()
    paths = ["/repos/x/src/**", "/repos/x/README.md", "/repos/x/src/**"]
    output = board.call("plan", {"path": PATH, "text": PLAN, "uses": [], "paths": paths}, stdin=True)
    assert board.read(output["path"].removeprefix("/") + ".json")["paths"] == ["/repos/x/src/**", "/repos/x/README.md"]


def test_a_planned_path_outside_the_change_s_mount_is_refused_and_one_inside_it_is_written(board: Board) -> None:
    board.write("changes/t.json", change())
    for outside in ("/repos/xy/src/**", "/repos/**"):
        refused = board.refusal("plan", {"path": PATH, "text": PLAN, "uses": [], "paths": [outside]}, stdin=True)
        assert refused["code"] == "changes::plan::outside_mount", outside
        assert refused["fields"]["paths"] == [outside]
    assert board.call("plan", {"path": PATH, "text": PLAN, "uses": [], "paths": ["/repos/x/*"]}, stdin=True)


def test_a_plan_record_whose_path_does_not_start_at_the_root_is_invalid_and_one_that_does_is_valid(
    board: Board,
) -> None:
    board.write("changes/t.json", change())
    output = board.call("plan", {"path": PATH, "text": PLAN, "uses": [], "paths": PATHS}, stdin=True)
    stored = output["path"].removeprefix("/") + ".json"
    written = board.read(stored)
    board.write(stored, json.dumps({**written, "paths": ["repos/x/**"]}))
    assert board.json("check", output["path"])["data"][0].get("code") == "board::record::invalid"
    board.write(stored, json.dumps(written))
    assert board.json("check", output["path"], "--healthy")["data"][0].get("code") is None
